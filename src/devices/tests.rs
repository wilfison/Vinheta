use super::*;

fn devices() -> Vec<Device> {
    [
        ("usb", "USB Microphone"),
        ("internal", "Internal Microphone"),
    ]
    .map(|(name, description)| Device {
        name: name.into(),
        description: description.into(),
    })
    .to_vec()
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

#[test]
fn the_system_default_comes_first() {
    let (entries, selected) = selector_entries(&[], "", None);
    assert_eq!(names(&entries), [""]);
    assert_eq!(selected, 0);
    assert!(entries[0].available);
}

#[test]
fn devices_keep_their_order() {
    let (entries, selected) = selector_entries(&devices(), "", None);
    assert_eq!(names(&entries), ["", "usb", "internal"]);
    assert_eq!(selected, 0);
    assert!(entries.iter().all(|entry| entry.available));
}

#[test]
fn the_chosen_device_is_selected() {
    let (entries, selected) = selector_entries(&devices(), "internal", None);
    assert_eq!(entries.len(), 3);
    assert_eq!(selected, 2);
}

#[test]
fn a_missing_chosen_device_is_added_as_unavailable() {
    let (entries, selected) = selector_entries(&devices(), "gone", None);
    assert_eq!(names(&entries), ["", "usb", "internal", "gone"]);
    assert_eq!(selected, 3);
    assert!(!entries[3].available);
    assert_eq!(entries[3].description, "gone");
}

#[test]
fn a_missing_device_keeps_its_last_description() {
    let (entries, selected) = selector_entries(&[], "gone", Some("Headset"));
    assert_eq!(entries[selected].description, "Headset");
    assert!(!entries[selected].available);
}

#[test]
fn the_system_default_is_never_missing() {
    assert!(!is_missing(&[], ""));
    assert!(!is_missing(&devices(), ""));
}

#[test]
fn a_chosen_device_is_missing_when_not_listed() {
    assert!(!is_missing(&devices(), "usb"));
    assert!(is_missing(&devices(), "gone"));
    assert!(is_missing(&[], "usb"));
}
