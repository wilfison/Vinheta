use super::*;

fn app(id: &str, categories: &[&str]) -> App {
    App {
        id: id.into(),
        name: id.to_uppercase(),
        categories: categories
            .iter()
            .map(|category| (*category).into())
            .collect(),
    }
}

fn apps() -> Vec<App> {
    vec![
        app("player", &["AudioVideo", "Audio", "Player"]),
        app("video", &["AudioVideo", "AudioVideoEditing"]),
        app("audacity", &["AudioVideo", "Audio", "AudioVideoEditing"]),
        app("other", &["AudioVideo", "Audio", "AudioVideoEditing"]),
    ]
}

fn ids(entries: &[App]) -> Vec<&str> {
    entries.iter().map(|app| app.id.as_str()).collect()
}

#[test]
fn an_editor_has_the_editing_category() {
    assert!(is_editor(&app("a", &["Audio", "AudioVideoEditing"])));
    assert!(!is_editor(&app("a", &["Audio", "Player"])));
    assert!(!is_editor(&app("a", &[])));
}

#[test]
fn a_video_editor_is_not_an_audio_editor() {
    assert!(!is_editor(&app(
        "a",
        &["Audio", "Video", "AudioVideoEditing"]
    )));
    // Kdenlive.
    assert!(!is_editor(&app("a", &["AudioVideo", "AudioVideoEditing"])));
}

#[test]
fn the_category_of_ardour_counts() {
    assert!(is_editor(&app(
        "a",
        &["AudioVideo", "Audio", "X-AudioEditing"]
    )));
}

#[test]
fn the_first_editor_is_picked_when_none_is_chosen() {
    let apps = apps();
    assert_eq!(pick(&apps, "").map(|app| app.id.as_str()), Some("audacity"));
}

#[test]
fn the_chosen_app_is_picked_even_when_it_is_not_an_editor() {
    let apps = apps();
    assert_eq!(
        pick(&apps, "player").map(|app| app.id.as_str()),
        Some("player")
    );
}

#[test]
fn a_chosen_app_that_is_gone_falls_back_to_the_first_editor() {
    let apps = apps();
    assert_eq!(
        pick(&apps, "removed").map(|app| app.id.as_str()),
        Some("audacity")
    );
}

#[test]
fn nothing_is_picked_without_an_editor() {
    let apps = [app("player", &["Audio", "Player"])];
    assert_eq!(pick(&apps, ""), None);
    assert_eq!(pick(&[], "removed"), None);
}

#[test]
fn the_selector_lists_the_editors_first() {
    let (entries, selected) = selector_entries(&apps(), "");
    assert_eq!(ids(&entries), ["audacity", "other", "player", "video"]);
    assert_eq!(selected, 0);
}

#[test]
fn the_selector_selects_the_chosen_app() {
    let (_, selected) = selector_entries(&apps(), "player");
    assert_eq!(selected, 3);
}

#[test]
fn the_selector_is_automatic_when_the_chosen_app_is_gone() {
    let (_, selected) = selector_entries(&apps(), "removed");
    assert_eq!(selected, 0);
}
