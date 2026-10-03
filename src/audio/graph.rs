/* graph.rs
 *
 * Copyright 2026 Will
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

use super::{Error, Event};

pub(super) const NODE_NAME: &str = "vinheta";
/// Playback streams whose node name starts with this are linked to the
/// virtual microphone as soon as their ports show up.
pub(super) const CALL_STREAM_PREFIX: &str = "vinheta-call-";

pub(super) enum Command {
    Quit,
}

struct Node {
    name: String,
    media_class: String,
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
                };
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

    /// The node to link as the microphone, and whether it is a fallback.
    fn resolve_mic(&self) -> Result<(u32, bool), Error> {
        let name = self
            .mic_override
            .as_ref()
            .or(self.default_source.as_ref())
            .ok_or(Error::NoMicrophone)?;

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

        self.nodes
            .iter()
            .find(|(id, node)| node.name == *name && Some(**id) != self.vinheta)
            .map(|(id, _)| (*id, false))
            .ok_or_else(|| Error::MicNotFound(name.clone()))
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
        let Some(vinheta) = self.vinheta else { return };

        let mut wanted = HashSet::new();
        for (id, node) in &self.nodes {
            if node.name.starts_with(CALL_STREAM_PREFIX) {
                wanted.extend(self.pairs(*id, vinheta));
            }
        }

        let mut mic_ready = None;
        match self.resolve_mic() {
            Ok((mic, fallback)) => {
                self.mic_error = None;
                let pairs = self.pairs(mic, vinheta);
                if !pairs.is_empty() {
                    mic_ready = Some((mic, fallback));
                }
                wanted.extend(pairs);
            }
            Err(error) => {
                if self.mic_error.as_ref() != Some(&error) {
                    self.mic_error = Some(error.clone());
                    self.emit(Event::Error(error));
                }
            }
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
            return Err(Error::PipeWire("connection lost".into()));
        }
        main_loop.run();
    }
    Ok(())
}

fn pipewire_error(error: pw::Error) -> Error {
    Error::PipeWire(error.to_string())
}

pub(super) fn run(
    mic: Option<String>,
    events: async_channel::Sender<Event>,
    commands: pw::channel::Receiver<Command>,
    started: mpsc::Sender<Result<(), Error>>,
) {
    if let Err(error) = run_loop(mic, events, commands, &started) {
        let _ = started.send(Err(error));
    }
}

fn run_loop(
    mic: Option<String>,
    events: async_channel::Sender<Event>,
    commands: pw::channel::Receiver<Command>,
    started: &mpsc::Sender<Result<(), Error>>,
) -> Result<(), Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_error)?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(pipewire_error)?;
    let core = context.connect_rc(None).map_err(pipewire_error)?;
    let registry = core.get_registry_rc().map_err(pipewire_error)?;

    let graph = Rc::new(RefCell::new(Graph {
        core: core.clone(),
        events: events.clone(),
        mic_override: mic,
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
            move |id, _, _, message| {
                if id == pw::core::PW_ID_CORE {
                    let error = Error::PipeWire(format!("connection lost: {message}"));
                    let _ = events.try_send(Event::Error(error));
                    main_loop.quit();
                }
            }
        })
        .register();

    let _commands = commands.attach(main_loop.loop_(), {
        let main_loop = main_loop.clone();
        move |command| match command {
            Command::Quit => main_loop.quit(),
        }
    });

    let _ = started.send(Ok(()));
    main_loop.run();
    Ok(())
}
