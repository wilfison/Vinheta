/* graph.rs
 *
 * Copyright 2026 wilfison
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! The PipeWire side of the engine. Everything here lives on one thread,
//! because PipeWire objects are not `Send`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc;

use pipewire as pw;
use pw::metadata::{Metadata, MetadataListener};
use pw::proxy::{ProxyListener, ProxyT};
use pw::registry::GlobalObject;
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

use super::{Device, Error, Event};

pub(super) const NODE_NAME: &str = "vinheta";
/// Playback streams whose node name starts with this are linked to the
/// virtual microphone as soon as their ports show up.
pub(super) const CALL_STREAM_PREFIX: &str = "vinheta-call-";
/// Monitor streams are routed by WirePlumber; the name is how the engine
/// finds the running ones when the monitor output changes.
pub(super) const MONITOR_STREAM_PREFIX: &str = "vinheta-monitor-";
const EPIPE: i32 = 32;

pub(super) enum Command {
    SetMic(Option<String>),
    SetIncludeVoice(bool),
    SetMonitor(Option<String>),
    Quit,
}

/// What the engine needs from `Config` on the PipeWire thread.
pub(super) struct Options {
    pub mic: Option<String>,
    pub include_voice: bool,
}

struct Node {
    name: String,
    media_class: String,
    description: Option<String>,
    nick: Option<String>,
}

/// The microphones and the outputs among `nodes`, each list sorted by
/// description. The virtual microphone is never one of them.
fn device_lists<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> (Vec<Device>, Vec<Device>) {
    let mut microphones = Vec::new();
    let mut outputs = Vec::new();
    for node in nodes {
        let list = match node.media_class.as_str() {
            "Audio/Source" | "Audio/Source/Virtual" if node.name != NODE_NAME => &mut microphones,
            "Audio/Sink" => &mut outputs,
            _ => continue,
        };
        if node.name.is_empty() {
            continue;
        }
        let description = [&node.description, &node.nick]
            .into_iter()
            .flatten()
            .find(|text| !text.is_empty())
            .unwrap_or(&node.name);
        list.push(Device {
            name: node.name.clone(),
            description: description.clone(),
        });
    }
    for list in [&mut microphones, &mut outputs] {
        list.sort_by_cached_key(|device| (device.description.to_lowercase(), device.name.clone()));
    }
    (microphones, outputs)
}

struct Port {
    node: u32,
    output: bool,
    channel: String,
}

struct Link {
    _proxy: pw::link::Link,
    _listener: ProxyListener,
}

struct Graph {
    core: pw::core::CoreRc,
    events: async_channel::Sender<Event>,
    mic_override: Option<String>,
    /// The chosen microphone that was reported as missing, to report it once.
    missing_mic: Option<String>,
    include_voice: bool,
    /// Set once the monitor output is changed while the engine runs. The
    /// inner `None` is the system default.
    monitor: Option<Option<String>>,
    /// The lists last sent in `Event::DevicesChanged`; `None` until the
    /// first registry roundtrips are over.
    devices: Option<(Vec<Device>, Vec<Device>)>,
    devices_ready: bool,
    default_source: Option<String>,
    nodes: HashMap<u32, Node>,
    ports: HashMap<u32, Port>,
    /// Global id of the virtual microphone, known once the server binds it.
    vinheta: Option<u32>,
    /// Links created by the engine, keyed by (output port, input port).
    links: HashMap<(u32, u32), Link>,
    linked_mic: Option<u32>,
    mic_error: Option<Error>,
    metadata: Option<(Metadata, MetadataListener)>,
}

impl Graph {
    fn emit(&self, event: Event) {
        let _ = self.events.try_send(event);
    }

    fn add_global(&mut self, global: &GlobalObject<&DictRef>) {
        let Some(props) = global.props else { return };
        match global.type_ {
            ObjectType::Node => {
                let node = Node {
                    name: props.get("node.name").unwrap_or_default().to_owned(),
                    media_class: props.get("media.class").unwrap_or_default().to_owned(),
                    description: props.get("node.description").map(str::to_owned),
                    nick: props.get("node.nick").map(str::to_owned),
                };
                // A playback that started just before the output was changed.
                if node.name.starts_with(MONITOR_STREAM_PREFIX) {
                    self.target_monitor(global.id);
                }
                self.nodes.insert(global.id, node);
            }
            ObjectType::Port => {
                let Some(node) = props.get("node.id").and_then(|id| id.parse().ok()) else {
                    return;
                };
                let port = Port {
                    node,
                    output: props.get("port.direction") == Some("out"),
                    channel: props.get("audio.channel").unwrap_or_default().to_owned(),
                };
                self.ports.insert(global.id, port);
            }
            _ => {}
        }
    }

    fn remove_global(&mut self, id: u32) {
        self.nodes.remove(&id);
        self.ports.remove(&id);
    }

    fn node_named(&self, name: &str) -> Option<u32> {
        self.nodes
            .iter()
            .find(|(id, node)| node.name == name && Some(**id) != self.vinheta)
            .map(|(id, _)| *id)
    }

    /// The node to link as the microphone, and whether it is a fallback. A
    /// chosen microphone that does not exist is reported once, and the
    /// default source is used until it shows up.
    fn resolve_mic(&mut self) -> Result<(u32, bool), Error> {
        let chosen = self.mic_override.clone().filter(|name| name != NODE_NAME);
        if let Some(name) = &chosen {
            if let Some(id) = self.node_named(name) {
                self.missing_mic = None;
                return Ok((id, false));
            }
            if self.missing_mic.as_ref() != Some(name) {
                self.missing_mic = Some(name.clone());
                self.emit(Event::Error(Error::MicNotFound(name.clone())));
            }
        }
        let fallback = self.mic_override.is_some();

        let name = self.default_source.as_ref().ok_or(Error::NoMicrophone)?;
        if name == NODE_NAME {
            // The user made the virtual microphone the system default, so the
            // default cannot be followed. "Audio/Source" excludes virtual sources.
            return self
                .nodes
                .iter()
                .filter(|(_, node)| node.media_class == "Audio/Source")
                .map(|(id, _)| (*id, true))
                .min()
                .ok_or(Error::NoMicrophone);
        }

        self.node_named(name)
            .map(|id| (id, fallback))
            .ok_or_else(|| Error::MicNotFound(name.clone()))
    }

    /// Asks WirePlumber to route a monitor stream to the chosen output. A
    /// removed key would leave the target the stream was created with, so
    /// the system default is asked for with "-1".
    fn target_monitor(&self, stream: u32) {
        if let (Some((metadata, _)), Some(monitor)) = (&self.metadata, &self.monitor) {
            let target = monitor.as_deref().unwrap_or("-1");
            metadata.set_property(stream, "target.object", None, Some(target));
        }
    }

    fn set_monitor(&mut self, name: Option<String>) {
        self.monitor = Some(name);
        for (id, node) in &self.nodes {
            if node.name.starts_with(MONITOR_STREAM_PREFIX) {
                self.target_monitor(*id);
            }
        }
    }

    fn set_mic(&mut self, name: Option<String>) {
        self.mic_override = name;
        self.missing_mic = None;
        self.sync();
    }

    fn set_include_voice(&mut self, enabled: bool) {
        if self.include_voice == enabled {
            return;
        }
        self.include_voice = enabled;
        if !enabled {
            self.voice_off();
        }
        self.sync();
    }

    fn voice_off(&mut self) {
        self.linked_mic = None;
        self.mic_error = None;
        self.missing_mic = None;
        self.emit(Event::MicUnlinked);
    }

    fn update_devices(&mut self) {
        if !self.devices_ready {
            return;
        }
        let devices = device_lists(self.nodes.values());
        if self.devices.as_ref() != Some(&devices) {
            self.devices = Some(devices.clone());
            self.emit(Event::DevicesChanged {
                microphones: devices.0,
                outputs: devices.1,
            });
        }
    }

    /// Port pairs that feed `source` into the virtual microphone. A mono
    /// source feeds every input, otherwise channels are matched by name.
    fn pairs(&self, source: u32, vinheta: u32) -> Vec<(u32, u32)> {
        let ports_of = |node: u32, output: bool| {
            self.ports
                .iter()
                .filter(move |(_, port)| port.node == node && port.output == output)
        };
        let outputs: Vec<_> = ports_of(source, true).collect();
        let mut pairs = Vec::new();
        for (output_id, output) in &outputs {
            for (input_id, input) in ports_of(vinheta, false) {
                if outputs.len() == 1 || output.channel == input.channel {
                    pairs.push((**output_id, *input_id));
                }
            }
        }
        pairs
    }

    /// Makes the links owned by the engine match the current graph.
    fn sync(&mut self) {
        self.update_devices();
        let Some(vinheta) = self.vinheta else { return };

        let mut wanted = HashSet::new();
        for (id, node) in &self.nodes {
            if node.name.starts_with(CALL_STREAM_PREFIX) {
                wanted.extend(self.pairs(*id, vinheta));
            }
        }

        let mut mic_ready = None;
        let mic = self.include_voice.then(|| self.resolve_mic());
        match mic {
            None => {}
            Some(Ok((mic, fallback))) => {
                self.mic_error = None;
                let pairs = self.pairs(mic, vinheta);
                if !pairs.is_empty() {
                    mic_ready = Some((mic, fallback));
                }
                wanted.extend(pairs);
            }
            Some(Err(error)) if self.mic_error.as_ref() != Some(&error) => {
                self.mic_error = Some(error.clone());
                self.emit(Event::Error(error));
            }
            Some(Err(_)) => {}
        }

        // Dropping a proxy destroys its link, since links do not linger.
        self.links.retain(|pair, _| wanted.contains(pair));
        for pair in wanted {
            if !self.links.contains_key(&pair) {
                self.create_link(pair);
            }
        }

        match mic_ready {
            Some((mic, fallback)) if self.linked_mic != Some(mic) => {
                self.linked_mic = Some(mic);
                let name = self.nodes[&mic].name.clone();
                self.emit(Event::MicLinked { name, fallback });
            }
            Some(_) => {}
            None => self.linked_mic = None,
        }
    }

    fn create_link(&mut self, (output, input): (u32, u32)) {
        let props = pw::properties::properties! {
            "link.output.node" => self.ports[&output].node.to_string(),
            "link.output.port" => output.to_string(),
            "link.input.node" => self.ports[&input].node.to_string(),
            "link.input.port" => input.to_string(),
        };
        match self
            .core
            .create_object::<pw::link::Link>("link-factory", &props)
        {
            Ok(proxy) => {
                let events = self.events.clone();
                let listener = proxy
                    .upcast_ref()
                    .add_listener_local()
                    .error(move |_, _, message| {
                        let error = Error::PipeWire(format!("link failed: {message}"));
                        let _ = events.try_send(Event::Error(error));
                    })
                    .register();
                let link = Link {
                    _proxy: proxy,
                    _listener: listener,
                };
                self.links.insert((output, input), link);
            }
            Err(error) => self.emit(Event::Error(Error::PipeWire(error.to_string()))),
        }
    }
}

/// Extracts `name` from the `{"name":"..."}` value of a default device key.
fn default_device_name(value: &str) -> Option<String> {
    let rest = value.split_once("\"name\"")?.1;
    let rest = rest.split_once('"')?.1;
    Some(rest.split_once('"')?.0.to_owned())
}

/// Runs the loop until the server has answered everything sent so far.
fn roundtrip(main_loop: &pw::main_loop::MainLoopRc, core: &pw::core::CoreRc) -> Result<(), Error> {
    let done = Rc::new(Cell::new(false));
    let failed = Rc::new(Cell::new(false));
    let pending = core.sync(0).map_err(pipewire_error)?;

    let _listener = core
        .add_listener_local()
        .done({
            let done = done.clone();
            let main_loop = main_loop.clone();
            move |id, seq| {
                if id == pw::core::PW_ID_CORE && seq == pending {
                    done.set(true);
                    main_loop.quit();
                }
            }
        })
        .error({
            let failed = failed.clone();
            let main_loop = main_loop.clone();
            move |id, _, _, _| {
                if id == pw::core::PW_ID_CORE {
                    failed.set(true);
                    main_loop.quit();
                }
            }
        })
        .register();

    while !done.get() {
        if failed.get() {
            return Err(Error::ConnectionLost("during the start".into()));
        }
        main_loop.run();
    }
    Ok(())
}

fn pipewire_error(error: pw::Error) -> Error {
    Error::PipeWire(error.to_string())
}

pub(super) fn run(
    options: Options,
    events: async_channel::Sender<Event>,
    commands: pw::channel::Receiver<Command>,
    started: mpsc::Sender<Result<(), Error>>,
) {
    if let Err(error) = run_loop(options, events, commands, &started) {
        let _ = started.send(Err(error));
    }
}

fn run_loop(
    options: Options,
    events: async_channel::Sender<Event>,
    commands: pw::channel::Receiver<Command>,
    started: &mpsc::Sender<Result<(), Error>>,
) -> Result<(), Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_error)?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(pipewire_error)?;
    let core = context
        .connect_rc(None)
        .map_err(|error| Error::Unreachable(error.to_string()))?;
    let registry = core.get_registry_rc().map_err(pipewire_error)?;

    let graph = Rc::new(RefCell::new(Graph {
        core: core.clone(),
        events: events.clone(),
        mic_override: options.mic,
        missing_mic: None,
        include_voice: options.include_voice,
        monitor: None,
        devices: None,
        devices_ready: false,
        default_source: None,
        nodes: HashMap::new(),
        ports: HashMap::new(),
        vinheta: None,
        links: HashMap::new(),
        linked_mic: None,
        mic_error: None,
        metadata: None,
    }));

    // The callbacks hold weak references: a strong one would keep the
    // connection, and so the node, alive after the engine is dropped.
    let _registry_listener = registry
        .add_listener_local()
        .global({
            let graph = Rc::downgrade(&graph);
            let registry = registry.downgrade();
            move |global| {
                let (Some(graph), Some(registry)) = (graph.upgrade(), registry.upgrade()) else {
                    return;
                };
                let is_default_metadata = global.type_ == ObjectType::Metadata
                    && global.props.and_then(|props| props.get("metadata.name")) == Some("default");
                if is_default_metadata {
                    if let Ok(metadata) = registry.bind::<Metadata, _>(global) {
                        let listener = metadata
                            .add_listener_local()
                            .property({
                                let graph = Rc::downgrade(&graph);
                                move |_, key, _, value| {
                                    if key == Some("default.audio.source") {
                                        if let Some(graph) = graph.upgrade() {
                                            let mut graph = graph.borrow_mut();
                                            graph.default_source =
                                                value.and_then(default_device_name);
                                            graph.sync();
                                        }
                                    }
                                    0
                                }
                            })
                            .register();
                        graph.borrow_mut().metadata = Some((metadata, listener));
                    }
                    return;
                }
                let mut graph = graph.borrow_mut();
                graph.add_global(global);
                graph.sync();
            }
        })
        .global_remove({
            let graph = Rc::downgrade(&graph);
            move |id| {
                if let Some(graph) = graph.upgrade() {
                    let mut graph = graph.borrow_mut();
                    graph.remove_global(id);
                    graph.sync();
                }
            }
        })
        .register();

    // One roundtrip lists the globals, the second delivers the metadata values.
    roundtrip(&main_loop, &core)?;
    roundtrip(&main_loop, &core)?;
    {
        let mut graph = graph.borrow_mut();
        graph.devices_ready = true;
        graph.update_devices();
    }

    if graph
        .borrow()
        .nodes
        .values()
        .any(|node| node.name == NODE_NAME)
    {
        return Err(Error::NodeExists);
    }

    // No object.linger: the node belongs to this connection and dies with it.
    let node = core
        .create_object::<pw::node::Node>(
            "adapter",
            &pw::properties::properties! {
                "factory.name" => "support.null-audio-sink",
                "node.name" => NODE_NAME,
                "node.description" => "Vinheta",
                "media.class" => "Audio/Source/Virtual",
                "audio.position" => "[ FL FR ]",
            },
        )
        .map_err(pipewire_error)?;
    let _node_listener = node
        .upcast_ref()
        .add_listener_local()
        .bound({
            let graph = Rc::downgrade(&graph);
            move |id| {
                if let Some(graph) = graph.upgrade() {
                    let mut graph = graph.borrow_mut();
                    graph.vinheta = Some(id);
                    graph.emit(Event::NodeCreated(id));
                    if !graph.include_voice {
                        graph.voice_off();
                    }
                    graph.sync();
                }
            }
        })
        .error({
            let events = events.clone();
            move |_, _, message| {
                let error = Error::PipeWire(format!("virtual microphone failed: {message}"));
                let _ = events.try_send(Event::Error(error));
            }
        })
        .register();

    let _core_listener = core
        .add_listener_local()
        .error({
            let main_loop = main_loop.clone();
            move |id, _, res, message| {
                // Other core errors are not fatal. The usual one is "unknown
                // resource": the server removed a link along with a stream
                // that ended, just before the engine dropped its proxy.
                if id == pw::core::PW_ID_CORE && res == -EPIPE {
                    let error = Error::ConnectionLost(message.to_string());
                    let _ = events.try_send(Event::Error(error));
                    main_loop.quit();
                }
            }
        })
        .register();

    let _commands = commands.attach(main_loop.loop_(), {
        let main_loop = main_loop.clone();
        let graph = Rc::downgrade(&graph);
        move |command| {
            let Some(graph) = graph.upgrade() else { return };
            match command {
                Command::SetMic(name) => graph.borrow_mut().set_mic(name),
                Command::SetIncludeVoice(enabled) => graph.borrow_mut().set_include_voice(enabled),
                Command::SetMonitor(name) => graph.borrow_mut().set_monitor(name),
                Command::Quit => main_loop.quit(),
            }
        }
    });

    let _ = started.send(Ok(()));
    main_loop.run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, media_class: &str, description: Option<&str>, nick: Option<&str>) -> Node {
        Node {
            name: name.into(),
            media_class: media_class.into(),
            description: description.map(Into::into),
            nick: nick.map(Into::into),
        }
    }

    fn names(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|device| device.name.as_str()).collect()
    }

    #[test]
    fn sources_and_sinks_are_split() {
        let nodes = [
            node("mic", "Audio/Source", Some("Mic"), None),
            node("filter", "Audio/Source/Virtual", Some("Filter"), None),
            node("phones", "Audio/Sink", Some("Phones"), None),
            node("player", "Stream/Output/Audio", Some("Player"), None),
            node("camera", "Video/Source", Some("Camera"), None),
        ];
        let (microphones, outputs) = device_lists(&nodes);
        assert_eq!(names(&microphones), ["filter", "mic"]);
        assert_eq!(names(&outputs), ["phones"]);
    }

    #[test]
    fn the_virtual_microphone_is_never_listed() {
        let nodes = [
            node(NODE_NAME, "Audio/Source/Virtual", Some("Vinheta"), None),
            node("mic", "Audio/Source", Some("Mic"), None),
        ];
        let (microphones, outputs) = device_lists(&nodes);
        assert_eq!(names(&microphones), ["mic"]);
        assert!(outputs.is_empty());
    }

    #[test]
    fn description_falls_back_to_nick_then_name() {
        let nodes = [
            node("a", "Audio/Sink", Some("Described"), Some("Nick")),
            node("b", "Audio/Sink", None, Some("Nick")),
            node("c", "Audio/Sink", Some(""), None),
        ];
        let (_, outputs) = device_lists(&nodes);
        let descriptions: Vec<_> = outputs.iter().map(|d| d.description.as_str()).collect();
        assert_eq!(descriptions, ["c", "Described", "Nick"]);
    }

    #[test]
    fn devices_are_sorted_by_description_then_name() {
        let nodes = [
            node("z", "Audio/Sink", Some("beta"), None),
            node("b", "Audio/Sink", Some("Alpha"), None),
            node("a", "Audio/Sink", Some("alpha"), None),
        ];
        let (_, outputs) = device_lists(&nodes);
        assert_eq!(names(&outputs), ["a", "b", "z"]);
    }

    #[test]
    fn nodes_without_a_name_are_skipped() {
        let nodes = [node("", "Audio/Sink", Some("Nameless"), None)];
        assert_eq!(device_lists(&nodes), (vec![], vec![]));
    }
}
