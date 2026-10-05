use super::*;

fn node(name: &str, media_class: &str, description: Option<&str>, nick: Option<&str>) -> Node {
    Node {
        name: name.into(),
        media_class: media_class.into(),
        description: description.map(Into::into),
        nick: nick.map(Into::into),
        recorder: None,
    }
}

fn recorder(app: &str, description: &str, meter: bool) -> Node {
    Node {
        recorder: Some(Recorder {
            app: app.into(),
            description: description.into(),
            meter,
        }),
        ..node(description, RECORDER_CLASS, None, None)
    }
}

fn names(devices: &[Device]) -> Vec<&str> {
    devices.iter().map(|device| device.name.as_str()).collect()
}

fn port(node: u32, output: bool) -> Port {
    Port { node, output }
}

#[test]
fn only_sinks_are_outputs() {
    let nodes = [
        node("mic", "Audio/Source", Some("Mic"), None),
        node("filter", "Audio/Source/Virtual", Some("Filter"), None),
        node("phones", "Audio/Sink", Some("Phones"), None),
        node("player", "Stream/Output/Audio", Some("Player"), None),
        node("drain", "", None, None),
    ];
    assert_eq!(names(&output_list(&nodes)), ["phones"]);
}

#[test]
fn description_falls_back_to_nick_then_name() {
    let nodes = [
        node("a", "Audio/Sink", Some("Described"), Some("Nick")),
        node("b", "Audio/Sink", None, Some("Nick")),
        node("c", "Audio/Sink", Some(""), None),
    ];
    let outputs = output_list(&nodes);
    let descriptions: Vec<_> = outputs.iter().map(|d| d.description.as_str()).collect();
    assert_eq!(descriptions, ["c", "Described", "Nick"]);
}

#[test]
fn outputs_are_sorted_by_description_then_name() {
    let nodes = [
        node("z", "Audio/Sink", Some("beta"), None),
        node("b", "Audio/Sink", Some("Alpha"), None),
        node("a", "Audio/Sink", Some("alpha"), None),
    ];
    assert_eq!(names(&output_list(&nodes)), ["a", "b", "z"]);
}

#[test]
fn nodes_without_a_name_are_skipped() {
    let nodes = [node("", "Audio/Sink", Some("Nameless"), None)];
    assert_eq!(output_list(&nodes), vec![]);
}

/// A microphone (1), a virtual one (2), an output (3), and the streams.
fn graph(streams: impl IntoIterator<Item = (u32, Node)>) -> HashMap<u32, Node> {
    let mut nodes = HashMap::from([
        (1, node("mic", "Audio/Source", None, None)),
        (2, node("filter", "Audio/Source/Virtual", None, None)),
        (3, node("phones", "Audio/Sink", None, None)),
    ]);
    nodes.extend(streams);
    nodes
}

#[test]
fn a_stream_fed_by_a_microphone_records() {
    let nodes = graph([
        (10, recorder("chrome", "Chrome", false)),
        (11, recorder("meet", "Meet", false)),
        (12, recorder("idle", "Idle", false)),
    ]);
    let found = recording_streams(&nodes, [(1, 10), (2, 11)]);
    assert_eq!(found, HashSet::from([10, 11]));
}

#[test]
fn a_stream_fed_by_an_output_does_not_record() {
    let nodes = graph([(10, recorder("obs", "OBS", false))]);
    assert!(recording_streams(&nodes, [(3, 10)]).is_empty());
}

#[test]
fn a_level_meter_does_not_record() {
    let nodes = graph([(10, recorder("gnome-control-center", "Settings", true))]);
    assert!(recording_streams(&nodes, [(1, 10)]).is_empty());
}

#[test]
fn a_stream_with_unknown_properties_does_not_record_yet() {
    let nodes = graph([(10, node("app", RECORDER_CLASS, None, None))]);
    assert!(recording_streams(&nodes, [(1, 10)]).is_empty());
}

#[test]
fn only_recording_streams_record() {
    let nodes = graph([(
        10,
        node("loopback", "Stream/Input/Audio/Internal", None, None),
    )]);
    assert!(recording_streams(&nodes, [(1, 10), (1, 3), (1, 99)]).is_empty());
}

#[test]
fn the_streams_of_an_app_are_one_target() {
    let streams = [
        recorder("chrome", "Google Chrome input", false),
        recorder("chrome", "Google Chrome input", false),
        recorder("audacity", "Audacity", false),
    ];
    let targets = target_list(&streams);
    assert_eq!(names(&targets), ["audacity", "chrome"]);
    assert_eq!(targets[1].description, "Google Chrome input");
}

#[test]
fn a_stream_without_an_app_is_no_target() {
    let streams = [
        recorder("", "", false),
        node("x", RECORDER_CLASS, None, None),
    ];
    assert!(target_list(&streams).is_empty());
}

#[test]
fn every_output_feeds_every_input_of_the_target() {
    let ports = HashMap::from([
        (1, port(10, true)),
        (2, port(20, false)),
        (3, port(20, false)),
        // The monitor port of the target and the ports of another node.
        (4, port(20, true)),
        (5, port(30, false)),
    ]);
    let mut pairs = call_pairs(&ports, 10, 20);
    pairs.sort_unstable();
    assert_eq!(pairs, [(1, 2), (1, 3)]);
}

#[test]
fn a_target_without_ports_gets_no_pair() {
    let ports = HashMap::from([(1, port(10, true))]);
    assert!(call_pairs(&ports, 10, 20).is_empty());
}
