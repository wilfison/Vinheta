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
use pw::metadata::Metadata;
use pw::node::{NodeChangeMask, NodeListener};
use pw::proxy::{ProxyListener, ProxyT};
use pw::registry::GlobalObject;
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

use super::{Device, Error, Event};

/// The private node every call stream is linked to, so that a playback
/// runs while no app is recording. Its media class keeps it out of every
/// device list; without one it would be an "Audio/Sink".
pub(super) const DRAIN_PREFIX: &str = "vinheta-drain-";
/// Playback streams whose node name starts with this and the id of the
/// process are linked to the recording apps as soon as their ports show up.
pub(super) const CALL_STREAM_PREFIX: &str = "vinheta-call-";
/// Monitor streams are routed by WirePlumber; the name is how the engine
/// finds the running ones when the monitor output changes.
pub(super) const MONITOR_STREAM_PREFIX: &str = "vinheta-monitor-";

/// The name of the streams of this process: another engine may be running.
pub(super) fn stream_prefix(prefix: &str) -> String {
    format!("{prefix}{}-", std::process::id())
}
const RECORDER_CLASS: &str = "Stream/Input/Audio";
const EPIPE: i32 = 32;

pub(super) enum Command {
    SetTarget(Option<String>),
    SetMonitor(Option<String>),
    Quit,
}

/// What the engine needs from `Config` on the PipeWire thread.
pub(super) struct Options {
    pub target: Option<String>,
}

struct Node {
    name: String,
    media_class: String,
    description: Option<String>,
    nick: Option<String>,
    /// Known once the server sent the properties of a recording stream:
    /// the registry does not carry the ones needed here.
    recorder: Option<Recorder>,
}

#[derive(Debug, Clone, PartialEq)]
struct Recorder {
    /// What identifies the app: the name of its binary when it has one.
    app: String,
    description: String,
    /// A level meter ("Peak detect" of a volume control), not a recording.
    meter: bool,
}

impl Recorder {
    fn new(props: &DictRef) -> Self {
        let get = |key| props.get(key).filter(|text: &&str| !text.is_empty());
        let name = get("application.name").or(get("node.name"));
        let app = get("application.process.binary")
            .or(name)
            .unwrap_or_default();
        Self {
            app: app.to_owned(),
            description: name.unwrap_or(app).to_owned(),
            meter: props.get("stream.monitor") == Some("true"),
        }
    }
}

/// The outputs among `nodes`, sorted by description.
fn output_list<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> Vec<Device> {
    let mut outputs = Vec::new();
    for node in nodes {
        if node.media_class != "Audio/Sink" || node.name.is_empty() {
            continue;
        }
        let description = [&node.description, &node.nick]
            .into_iter()
            .flatten()
            .find(|text| !text.is_empty())
            .unwrap_or(&node.name);
        outputs.push(Device {
            name: node.name.clone(),
            description: description.clone(),
        });
    }
    outputs.sort_by_cached_key(|device| (device.description.to_lowercase(), device.name.clone()));
    outputs
}

/// The streams that record a microphone, given the links of the graph as
/// (output node, input node). A stream fed by the monitor of an output
/// (a desktop recording) and a level meter are not among them.
fn recording_streams(
    nodes: &HashMap<u32, Node>,
    links: impl IntoIterator<Item = (u32, u32)>,
) -> HashSet<u32> {
    let is_microphone = |id: &u32| {
        nodes.get(id).is_some_and(|node| {
            matches!(
                node.media_class.as_str(),
                "Audio/Source" | "Audio/Source/Virtual"
            )
        })
    };
    let is_recorder = |id: &u32| {
        nodes.get(id).is_some_and(|node| {
            node.media_class == RECORDER_CLASS
                && node
                    .recorder
                    .as_ref()
                    .is_some_and(|recorder| !recorder.meter)
        })
    };
    links
        .into_iter()
        .filter(|(output, input)| is_microphone(output) && is_recorder(input))
        .map(|(_, input)| input)
        .collect()
}

/// One entry per app among the recording `streams`, sorted by description.
fn target_list<'a>(streams: impl IntoIterator<Item = &'a Node>) -> Vec<Device> {
    let mut targets: Vec<Device> = Vec::new();
    for recorder in streams
        .into_iter()
        .filter_map(|node| node.recorder.as_ref())
    {
        if recorder.app.is_empty() || targets.iter().any(|target| target.name == recorder.app) {
            continue;
        }
        targets.push(Device {
            name: recorder.app.clone(),
            description: recorder.description.clone(),
        });
    }
    targets.sort_by_cached_key(|target| (target.description.to_lowercase(), target.name.clone()));
    targets
}

struct Port {
    node: u32,
    output: bool,
}

/// Port pairs that feed `source` into `target`: every output into every
/// input, since the call branch is mono.
fn call_pairs(ports: &HashMap<u32, Port>, source: u32, target: u32) -> Vec<(u32, u32)> {
    let ports_of = |node: u32, output: bool| {
        ports
            .iter()
            .filter(move |(_, port)| port.node == node && port.output == output)
            .map(|(id, _)| *id)
    };
    let mut pairs = Vec::new();
    for output in ports_of(source, true) {
        pairs.extend(ports_of(target, false).map(|input| (output, input)));
    }
    pairs
}

struct Link {
    _proxy: pw::link::Link,
    _listener: ProxyListener,
}

/// Keeps the properties of a recording stream coming.
struct Stream {
    _proxy: pw::node::Node,
    _listener: NodeListener,
}

struct Graph {
    core: pw::core::CoreRc,
    events: async_channel::Sender<Event>,
    call_prefix: String,
    monitor_prefix: String,
    /// The app the sounds are sent to. `None` is every recording app.
    target: Option<String>,
    /// Set once the monitor output is changed while the engine runs. The
    /// inner `None` is the system default.
    monitor: Option<Option<String>>,
    /// The lists last sent in `Event::DevicesChanged` and
    /// `Event::TargetsChanged`; `None` until the first registry roundtrips
    /// are over.
    outputs: Option<Vec<Device>>,
    targets: Option<Vec<Device>>,
    lists_ready: bool,
    nodes: HashMap<u32, Node>,
    ports: HashMap<u32, Port>,
    /// Every link of the graph, as (output node, input node).
    node_links: HashMap<u32, (u32, u32)>,
    streams: HashMap<u32, Stream>,
    /// The streams seen recording a microphone. One stays here until it is
    /// removed, so a stream that WirePlumber links again keeps the sounds.
    recorders: HashSet<u32>,
    /// Global id of the drain node, known once the server binds it.
    drain: Option<u32>,
    /// Links created by the engine, keyed by (output port, input port).
    links: HashMap<(u32, u32), Link>,
    metadata: Option<Metadata>,
}

impl Graph {
    fn emit(&self, event: Event) {
        let _ = self.events.try_send(event);
    }

    /// Returns whether the global is a recording stream, whose properties
    /// are then to be asked for.
    fn add_global(&mut self, global: &GlobalObject<&DictRef>) -> bool {
        let Some(props) = global.props else {
            return false;
        };
        let id = |key| props.get(key).and_then(|id| id.parse().ok());
        match global.type_ {
            ObjectType::Node => {
                let node = Node {
                    name: props.get("node.name").unwrap_or_default().to_owned(),
                    media_class: props.get("media.class").unwrap_or_default().to_owned(),
                    description: props.get("node.description").map(str::to_owned),
                    nick: props.get("node.nick").map(str::to_owned),
                    recorder: None,
                };
                // A playback that started just before the output was changed.
                if node.name.starts_with(&self.monitor_prefix) {
                    self.target_monitor(global.id);
                }
                let records = node.media_class == RECORDER_CLASS;
                self.nodes.insert(global.id, node);
                return records;
            }
            ObjectType::Port => {
                if let Some(node) = id("node.id") {
                    let output = props.get("port.direction") == Some("out");
                    self.ports.insert(global.id, Port { node, output });
                }
            }
            ObjectType::Link => {
                if let (Some(output), Some(input)) = (id("link.output.node"), id("link.input.node"))
                {
                    self.node_links.insert(global.id, (output, input));
                }
            }
            _ => {}
        }
        false
    }

    fn remove_global(&mut self, id: u32) {
        self.nodes.remove(&id);
        self.ports.remove(&id);
        self.node_links.remove(&id);
        self.streams.remove(&id);
    }

    fn set_recorder(&mut self, id: u32, recorder: Recorder) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.recorder.as_ref() != Some(&recorder) {
                node.recorder = Some(recorder);
                self.sync();
            }
        }
    }

    /// Asks WirePlumber to route a monitor stream to the chosen output. A
    /// removed key would leave the target the stream was created with, so
    /// the system default is asked for with "-1".
    fn target_monitor(&self, stream: u32) {
        if let (Some(metadata), Some(monitor)) = (&self.metadata, &self.monitor) {
            let target = monitor.as_deref().unwrap_or("-1");
            metadata.set_property(stream, "target.object", None, Some(target));
        }
    }

    fn set_monitor(&mut self, name: Option<String>) {
        self.monitor = Some(name);
        for (id, node) in &self.nodes {
            if node.name.starts_with(&self.monitor_prefix) {
                self.target_monitor(*id);
            }
        }
    }

    fn set_target(&mut self, app: Option<String>) {
        self.target = app;
        self.sync();
    }

    fn update_lists(&mut self) {
        let found = recording_streams(&self.nodes, self.node_links.values().copied());
        self.recorders.extend(found);
        self.recorders.retain(|id| self.nodes.contains_key(id));
        if !self.lists_ready {
            return;
        }

        let outputs = output_list(self.nodes.values());
        if self.outputs.as_ref() != Some(&outputs) {
            self.outputs = Some(outputs.clone());
            self.emit(Event::DevicesChanged { outputs });
        }
        let targets = target_list(self.recorders.iter().map(|id| &self.nodes[id]));
        if self.targets.as_ref() != Some(&targets) {
            self.targets = Some(targets.clone());
            self.emit(Event::TargetsChanged(targets));
        }
    }

    /// Whether the sounds are to be sent to this recording stream.
    fn is_target(&self, stream: u32) -> bool {
        let app = self.nodes[&stream].recorder.as_ref().map(|r| &r.app);
        self.target.is_none() || self.target.as_ref() == app
    }

    /// Makes the links owned by the engine match the current graph.
    fn sync(&mut self) {
        self.update_lists();
        let Some(drain) = self.drain else { return };

        let mut wanted = HashSet::new();
        for (id, node) in &self.nodes {
            if !node.name.starts_with(&self.call_prefix) {
                continue;
            }
            wanted.extend(call_pairs(&self.ports, *id, drain));
            for stream in self
                .recorders
                .iter()
                .filter(|stream| self.is_target(**stream))
            {
                wanted.extend(call_pairs(&self.ports, *id, *stream));
            }
        }

        // Dropping a proxy destroys its link, since links do not linger.
        self.links.retain(|pair, _| wanted.contains(pair));
        for pair in wanted {
            if !self.links.contains_key(&pair) {
                self.create_link(pair);
            }
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
        call_prefix: stream_prefix(CALL_STREAM_PREFIX),
        monitor_prefix: stream_prefix(MONITOR_STREAM_PREFIX),
        target: options.target,
        monitor: None,
        outputs: None,
        targets: None,
        lists_ready: false,
        nodes: HashMap::new(),
        ports: HashMap::new(),
        node_links: HashMap::new(),
        streams: HashMap::new(),
        recorders: HashSet::new(),
        drain: None,
        links: HashMap::new(),
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
                    graph.borrow_mut().metadata = registry.bind::<Metadata, _>(global).ok();
                    return;
                }
                let records = graph.borrow_mut().add_global(global);
                if records {
                    if let Ok(proxy) = registry.bind::<pw::node::Node, _>(global) {
                        let id = global.id;
                        let listener = proxy
                            .add_listener_local()
                            .info({
                                let graph = Rc::downgrade(&graph);
                                move |info| {
                                    // An update of anything else comes
                                    // with no properties at all.
                                    if !info.change_mask().contains(NodeChangeMask::PROPS) {
                                        return;
                                    }
                                    if let (Some(graph), Some(props)) =
                                        (graph.upgrade(), info.props())
                                    {
                                        graph.borrow_mut().set_recorder(id, Recorder::new(props));
                                    }
                                }
                            })
                            .register();
                        let stream = Stream {
                            _proxy: proxy,
                            _listener: listener,
                        };
                        graph.borrow_mut().streams.insert(id, stream);
                    }
                }
                graph.borrow_mut().sync();
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

    // One roundtrip lists the globals, the second delivers the properties
    // of the recording streams.
    roundtrip(&main_loop, &core)?;
    roundtrip(&main_loop, &core)?;
    {
        let mut graph = graph.borrow_mut();
        graph.lists_ready = true;
        graph.update_lists();
    }

    // No object.linger: the node belongs to this connection and dies with it.
    let node = core
        .create_object::<pw::node::Node>(
            "adapter",
            &pw::properties::properties! {
                "factory.name" => "support.null-audio-sink",
                "node.name" => format!("{DRAIN_PREFIX}{}", std::process::id()),
                "media.class" => "Audio/Sink/Internal",
                "audio.position" => "[ MONO ]",
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
                    graph.drain = Some(id);
                    graph.emit(Event::Ready);
                    graph.sync();
                }
            }
        })
        .error({
            let events = events.clone();
            move |_, _, message| {
                let error = Error::PipeWire(format!("drain node failed: {message}"));
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
                Command::SetTarget(app) => graph.borrow_mut().set_target(app),
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
mod tests;
