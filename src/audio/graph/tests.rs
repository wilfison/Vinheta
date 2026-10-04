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
