use super::*;

fn parse(text: &str) -> PadStore {
    PadStore::parse(text).unwrap()
}

fn pad(fields: &str) -> PadSettings {
    parse(&format!(
        r#"{{"version": 1, "pads": {{"/a.wav": {{{fields}}}}}}}"#
    ))
    .get("/a.wav")
}

#[test]
fn a_pad_without_an_entry_has_the_default_settings() {
    let settings = PadStore::default().get("/a.wav");
    assert_eq!(settings, PadSettings::default());
    assert_eq!(settings.volume, 1.0);
    assert!(!settings.looping && settings.name.is_none() && settings.color.is_none());
}

#[test]
fn default_settings_remove_the_entry() {
    let mut store = PadStore::default();
    store.set(
        "/a.wav",
        PadSettings {
            looping: true,
            ..Default::default()
        },
    );
    assert!(!store.is_empty());
    store.set("/a.wav", PadSettings::default());
    assert!(store.is_empty());
}

#[test]
fn every_field_is_read() {
    let settings = pad(r#""name": "Intro", "color": "purple", "volume": 0.8, "loop": true"#);
    assert_eq!(
        settings,
        PadSettings {
            name: Some("Intro".into()),
            color: Some(PadColor::Purple),
            volume: 0.8,
            looping: true,
            favorite: false,
            shortcut: None,
            background: None,
        }
    );
}

#[test]
fn the_shortcut_is_read_and_left_out_when_none() {
    assert_eq!(pad(r#""shortcut": "q""#).shortcut, Some('q'));
    assert_eq!(pad(r#""shortcut": "Q""#).shortcut, Some('q'));
    assert_eq!(pad(r#""shortcut": "7""#).shortcut, Some('7'));
    assert_eq!(pad(r#""loop": true"#).shortcut, None);
    assert_eq!(pad(r#""shortcut": 7, "loop": true"#).shortcut, None);
    assert_eq!(pad(r#""shortcut": "qw", "loop": true"#).shortcut, None);
    assert_eq!(pad(r#""shortcut": "", "loop": true"#).shortcut, None);
    assert_eq!(pad(r#""shortcut": "-", "loop": true"#).shortcut, None);

    let mut store = PadStore::default();
    let keyed = PadSettings {
        shortcut: Some('q'),
        ..Default::default()
    };
    store.set("/a.wav", keyed.clone());
    let looping = PadSettings {
        looping: true,
        ..Default::default()
    };
    store.set("/b.wav", looping);
    let text = serde_json::to_string(&File {
        version: VERSION,
        pads: &store.pads,
    })
    .unwrap();
    assert_eq!(text.matches("\"shortcut\":\"q\"").count(), 1);
    assert_eq!(text.matches("shortcut").count(), 1);
    assert_eq!(parse(&text).get("/a.wav"), keyed);
}

fn folder_store() -> PadStore {
    parse(
        r#"{"version": 1, "pads": {
            "/a/F/one.wav": {"color": "red", "shortcut": "q"},
            "/a/F/sub/two.wav": {"name": "Two"},
            "/a/Fx/three.wav": {"shortcut": "w"},
            "/b/G/one.wav": {"shortcut": "e", "loop": true}
        }}"#,
    )
}

#[test]
fn move_folder_moves_the_entries_inside_it() {
    let mut store = folder_store();
    assert_eq!(store.move_folder("/a/F", "/c/New"), 2);
    assert_eq!(store.get("/a/F/one.wav"), PadSettings::default());
    let one = store.get("/c/New/one.wav");
    assert_eq!(one.color, Some(PadColor::Red));
    assert_eq!(one.shortcut, Some('q'));
    assert_eq!(store.get("/c/New/sub/two.wav").name.as_deref(), Some("Two"));
}

#[test]
fn move_folder_respects_the_path_boundary() {
    let mut store = folder_store();
    store.move_folder("/a/F/", "/c/New");
    assert_eq!(store.get("/a/Fx/three.wav").shortcut, Some('w'));
    assert_eq!(store.shortcut_owner('w'), Some("/a/Fx/three.wav"));
}

#[test]
fn move_folder_replaces_the_destination_and_keeps_keys_unique() {
    let mut store = folder_store();
    assert_eq!(store.move_folder("/a/F", "/b/G"), 2);
    let one = store.get("/b/G/one.wav");
    assert_eq!(one.shortcut, Some('q'));
    assert!(!one.looping);
    assert_eq!(store.shortcut_owner('e'), None);
    assert_eq!(store.shortcut_owner('q'), Some("/b/G/one.wav"));
}

#[test]
fn move_folder_to_itself_changes_nothing() {
    let mut store = folder_store();
    assert_eq!(store.move_folder("/a/F", "/a/F/"), 0);
    assert_eq!(store, folder_store());
}

#[test]
fn shortcut_keys() {
    assert_eq!(shortcut_key('q'), Some('q'));
    assert_eq!(shortcut_key('Q'), Some('q'));
    assert_eq!(shortcut_key('7'), Some('7'));
    assert_eq!(shortcut_key('é'), None);
    assert_eq!(shortcut_key(' '), None);
    assert_eq!(shortcut_key('-'), None);
    assert_eq!(shortcut_label('q'), "Q");
    assert_eq!(shortcut_label('7'), "7");
}

fn keyed(key: char) -> PadSettings {
    PadSettings {
        shortcut: Some(key),
        ..Default::default()
    }
}

#[test]
fn a_shortcut_is_taken_from_the_others() {
    let mut store = PadStore::default();
    assert_eq!(store.shortcut_owner('q'), None);
    assert!(store.take_shortcut('q', "/a.wav").is_empty());

    store.set("/b.wav", keyed('q'));
    assert_eq!(store.shortcut_owner('Q'), Some("/b.wav"));
    assert_eq!(store.take_shortcut('q', "/a.wav"), ["/b.wav"]);
    assert_eq!(store.shortcut_owner('q'), None);
    // The entry had nothing else.
    assert!(store.is_empty());

    store.set("/a.wav", keyed('q'));
    assert!(store.take_shortcut('q', "/a.wav").is_empty());
    assert_eq!(store.shortcut_owner('q'), Some("/a.wav"));
}

#[test]
fn load_keeps_a_shortcut_unique() {
    let store = parse(
        r#"{"version": 1, "pads": {
            "/c.wav": {"shortcut": "q"},
            "/a.wav": {"shortcut": "Q", "loop": true},
            "/b.wav": {"shortcut": "q", "loop": true},
            "/d.wav": {"shortcut": "w"}
        }}"#,
    );
    assert_eq!(store.get("/a.wav").shortcut, Some('q'));
    assert_eq!(store.get("/b.wav").shortcut, None);
    assert!(store.get("/b.wav").looping);
    assert_eq!(store.get("/d.wav").shortcut, Some('w'));
    // Nothing is left of an entry that only had the key.
    assert_eq!(store.pads.len(), 3);
}

#[test]
fn rename_carries_the_shortcut() {
    let mut store = PadStore::default();
    store.set("/a.wav", keyed('q'));
    store.set("/b.wav", keyed('w'));
    assert!(store.rename("/a.wav", "/b.wav"));
    assert_eq!(store.shortcut_owner('q'), Some("/b.wav"));
    assert_eq!(store.shortcut_owner('w'), None);
}

#[test]
fn the_favorite_is_read_and_left_out_when_false() {
    assert!(pad(r#""favorite": true"#).favorite);
    assert!(!pad(r#""loop": true"#).favorite);
    assert!(!pad(r#""favorite": "yes", "loop": true"#).favorite);
    let mut store = PadStore::default();
    let favorite = PadSettings {
        favorite: true,
        ..Default::default()
    };
    store.set("/a.wav", favorite.clone());
    let looping = PadSettings {
        looping: true,
        ..Default::default()
    };
    store.set("/b.wav", looping);
    let text = serde_json::to_string(&File {
        version: VERSION,
        pads: &store.pads,
    })
    .unwrap();
    assert_eq!(text.matches("\"favorite\":true").count(), 1);
    assert_eq!(parse(&text).get("/a.wav"), favorite);
}

#[test]
fn rename_moves_the_settings() {
    let looping = PadSettings {
        looping: true,
        ..Default::default()
    };
    let favorite = PadSettings {
        favorite: true,
        ..Default::default()
    };
    let mut store = PadStore::default();
    assert!(!store.rename("/a.wav", "/b.wav"));
    assert!(store.is_empty());

    store.set("/a.wav", looping.clone());
    assert!(store.rename("/a.wav", "/b.wav"));
    assert_eq!(store.get("/a.wav"), PadSettings::default());
    assert_eq!(store.get("/b.wav"), looping);

    store.set("/c.wav", favorite);
    assert!(store.rename("/b.wav", "/c.wav"));
    assert_eq!(store.get("/c.wav"), looping);
    assert_eq!(store.pads.len(), 1);
}

#[test]
fn a_missing_file_is_an_empty_store() {
    let file = std::env::temp_dir().join("vinheta-test-missing/pads.json");
    assert_eq!(PadStore::load(&file), Ok(PadStore::default()));
}

fn with_background(name: &str) -> PadSettings {
    PadSettings {
        background: Some(name.into()),
        ..Default::default()
    }
}

#[test]
fn the_background_is_read_and_left_out_when_none() {
    assert_eq!(
        pad(r#""background": "0123abcd.jpg""#).background.as_deref(),
        Some("0123abcd.jpg")
    );
    assert_eq!(pad(r#""loop": true"#).background, None);
    assert_eq!(PadSettings::default().background, None);
    let mut store = PadStore::default();
    store.set("/a.wav", with_background("0123abcd.jpg"));
    store.set("/b.wav", keyed('q'));
    let text = serde_json::to_string(&File {
        version: VERSION,
        pads: &store.pads,
    })
    .unwrap();
    assert_eq!(text.matches("\"background\"").count(), 1);
    assert_eq!(parse(&text), store);
}

#[test]
fn a_background_is_a_name_inside_the_directory() {
    for name in ["", "../pads.json", "/etc/passwd", "a/b.jpg", ".hidden.jpg"] {
        let json = serde_json::to_string(name).unwrap();
        let settings = pad(&format!(r#""background": {json}, "loop": true"#));
        assert_eq!(settings.background, None, "{name}");
    }
    assert_eq!(pad(r#""background": 7, "loop": true"#).background, None);
}

#[test]
fn rename_and_move_folder_carry_the_background() {
    let mut store = PadStore::default();
    store.set("/a/x.wav", with_background("one.jpg"));
    assert!(store.rename("/a/x.wav", "/a/y.wav"));
    assert_eq!(store.get("/a/y.wav").background.as_deref(), Some("one.jpg"));
    assert_eq!(store.move_folder("/a", "/b"), 1);
    assert_eq!(store.get("/b/y.wav").background.as_deref(), Some("one.jpg"));
}

#[test]
fn backgrounds_lists_each_name_once() {
    let mut store = PadStore::default();
    store.set("/a.wav", with_background("one.jpg"));
    store.set("/b.wav", with_background("one.jpg"));
    store.set("/c.wav", with_background("two.png"));
    store.set("/d.wav", keyed('q'));
    let names: Vec<String> = store.backgrounds().into_iter().collect();
    assert_eq!(names, ["one.jpg", "two.png"]);
}

#[test]
fn unknown_fields_are_ignored() {
    let store =
        parse(r#"{"version": 1, "later": 1, "pads": {"/a.wav": {"loop": true, "hotkey": "F1"}}}"#);
    assert!(store.get("/a.wav").looping);
}

#[test]
fn the_volume_is_clamped() {
    assert_eq!(pad(r#""volume": 7"#).volume, 1.0);
    assert_eq!(pad(r#""volume": -1, "loop": true"#).volume, 0.0);
}

#[test]
fn a_bad_field_resets_that_field_only() {
    let settings = pad(r#""color": "pink", "loop": true"#);
    assert_eq!(settings.color, None);
    assert!(settings.looping);
    let settings = pad(r#""name": 3, "volume": "loud", "loop": "yes", "color": "red""#);
    assert_eq!(
        settings,
        PadSettings {
            color: Some(PadColor::Red),
            ..Default::default()
        }
    );
}

#[test]
fn an_empty_name_is_no_name() {
    assert_eq!(pad(r#""name": "", "loop": true"#).name, None);
    assert_eq!(pad(r#""name": "   ", "loop": true"#).name, None);
    assert_eq!(pad(r#""name": " Intro ""#).name.as_deref(), Some("Intro"));
}

#[test]
fn a_broken_file_is_an_error() {
    assert!(matches!(PadStore::parse("{"), Err(LoadError::Invalid(_))));
    assert!(matches!(PadStore::parse("[]"), Err(LoadError::Invalid(_))));
    assert_eq!(
        PadStore::parse(r#"{"version": 2, "pads": {}}"#),
        Err(LoadError::Version(2))
    );
}

#[test]
fn prune_removes_only_files_gone_from_a_folder_that_exists() {
    let directory = std::env::temp_dir().join(format!("vinheta-test-prune-{}", std::process::id()));
    let folder = directory.join("Sounds");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("here.wav"), "").unwrap();
    let path = |name: &str| folder.join(name).to_str().unwrap().to_owned();
    let elsewhere = directory
        .join("Missing/away.wav")
        .to_str()
        .unwrap()
        .to_owned();
    let mut store = PadStore::default();
    for entry in [path("here.wav"), path("deleted.wav"), elsewhere.clone()] {
        store.set(&entry, pad(r#""loop": true"#));
    }
    let removed = store.prune_missing();
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(store.get(&path("deleted.wav")), PadSettings::default());
    assert!(store.get(&path("here.wav")).looping);
    assert!(store.get(&elsewhere).looping);
}

#[test]
fn a_saved_store_loads_back() {
    let directory = std::env::temp_dir().join(format!("vinheta-test-{}", std::process::id()));
    let file = directory.join("nested/pads.json");
    let mut store = PadStore::default();
    store.set(
        "/sounds/a b.wav",
        PadSettings {
            name: Some("Intro “1”".into()),
            color: Some(PadColor::Brown),
            volume: 0.25,
            looping: true,
            favorite: true,
            shortcut: Some('q'),
            background: Some("0123abcd.jpg".into()),
        },
    );
    store.set(
        "/sounds/c.ogg",
        PadSettings {
            color: Some(PadColor::Blue),
            ..Default::default()
        },
    );
    store.save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    let loaded = PadStore::load(&file);
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(loaded, Ok(store));
    // Fields at their default are left out.
    assert_eq!(text.matches("\"volume\"").count(), 1);
    assert_eq!(text.matches("\"loop\"").count(), 1);
    assert!(text.contains("\"version\": 1"));
}

#[test]
fn color_names_are_stable() {
    let names: Vec<_> = PadColor::ALL.iter().map(|color| color.name()).collect();
    assert_eq!(
        names,
        ["blue", "green", "yellow", "orange", "red", "purple", "brown"]
    );
    assert_eq!(PadColor::from_name("purple"), Some(PadColor::Purple));
    assert_eq!(PadColor::from_name("Purple"), None);
}

#[test]
fn trigger_mode_names() {
    assert_eq!(TriggerMode::from_name("overlap"), TriggerMode::Overlap);
    assert_eq!(TriggerMode::from_name("restart"), TriggerMode::Restart);
    assert_eq!(
        TriggerMode::from_name("stop-others"),
        TriggerMode::StopOthers
    );
    assert_eq!(TriggerMode::from_name("bogus"), TriggerMode::Overlap);
}

#[test]
fn trigger_rule() {
    assert_eq!(trigger(TriggerMode::Overlap, true), Trigger::Stop);
    assert_eq!(trigger(TriggerMode::Overlap, false), Trigger::Start);
    assert_eq!(trigger(TriggerMode::Restart, true), Trigger::Restart);
    assert_eq!(trigger(TriggerMode::Restart, false), Trigger::Start);
    assert_eq!(trigger(TriggerMode::StopOthers, true), Trigger::Stop);
    assert_eq!(trigger(TriggerMode::StopOthers, false), Trigger::StartAlone);
}

#[test]
fn play_rule_never_stops() {
    assert_eq!(play(TriggerMode::Overlap, true), None);
    assert_eq!(play(TriggerMode::Overlap, false), Some(Trigger::Start));
    assert_eq!(play(TriggerMode::Restart, true), Some(Trigger::Restart));
    assert_eq!(play(TriggerMode::Restart, false), Some(Trigger::Start));
    assert_eq!(play(TriggerMode::StopOthers, true), None);
    assert_eq!(
        play(TriggerMode::StopOthers, false),
        Some(Trigger::StartAlone)
    );
}

#[test]
fn search_match() {
    assert!(matches("horn", "Air Horn", "Air Horn.wav"));
    assert!(matches("horn air", "Air Horn", "Air Horn.wav"));
    assert!(matches("  AIR   hOrN ", "Air Horn", "Air Horn.wav"));
    // Only the file name has it.
    assert!(matches("crick", "Zebra", "Crickets.wav"));
    // One word in each.
    assert!(matches("zeb crick", "Zebra", "Crickets.wav"));
    assert!(matches("é", "CAFÉ", "x.wav"));
    assert!(matches("CAFÉ", "café", "x.wav"));
    assert!(!matches("bell", "Air Horn", "Air Horn.wav"));
    assert!(!matches("air bell", "Air Horn", "Air Horn.wav"));
    assert!(!matches("", "Air Horn", "Air Horn.wav"));
    assert!(!matches("   ", "Air Horn", "Air Horn.wav"));
}

#[test]
fn sort_order_names() {
    assert_eq!(SortOrder::from_name("name"), SortOrder::Name);
    assert_eq!(SortOrder::from_name("recent"), SortOrder::Recent);
    assert_eq!(SortOrder::from_name("bogus"), SortOrder::Name);
    assert_eq!(SortOrder::Recent.name(), "recent");
}

fn key<'a>(display_name: &'a str, file_name: &'a str, seconds: u64) -> SortKey<'a> {
    SortKey {
        display_name,
        file_name,
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
    }
}

#[test]
fn sort_by_name() {
    let order = SortOrder::Name;
    let less = |a, b| compare(order, a, b) == Ordering::Less;
    assert!(less(
        key("apple", "apple.wav", 1),
        key("Bell", "Bell.wav", 9)
    ));
    assert!(less(key("Bell", "Bell.ogg", 1), key("bell", "bell.mp3", 1)));
    assert_eq!(
        compare(order, key("a", "a.wav", 1), key("a", "a.wav", 2)),
        Ordering::Equal
    );
    // A custom name moves the pad.
    assert!(less(
        key("Air Horn", "Air Horn.wav", 1),
        key("Crickets", "Crickets.wav", 1)
    ));
    assert!(less(
        key("Crickets", "Crickets.wav", 1),
        key("Zulu", "Air Horn.wav", 1)
    ));
}

#[test]
fn sort_by_recent() {
    let order = SortOrder::Recent;
    let less = |a, b| compare(order, a, b) == Ordering::Less;
    assert!(less(key("z", "z.wav", 9), key("a", "a.wav", 1)));
    // The same time falls back to the name, then to the file name.
    assert!(less(key("a", "z.wav", 5), key("b", "a.wav", 5)));
    assert!(less(key("a", "a.ogg", 5), key("a", "a.wav", 5)));
}

#[test]
fn times() {
    assert_eq!(format_time(Duration::ZERO), "00:00");
    assert_eq!(format_time(Duration::from_secs_f64(59.9)), "00:59");
    assert_eq!(format_time(Duration::from_secs(60)), "01:00");
    assert_eq!(format_time(Duration::from_secs(72)), "01:12");
    assert_eq!(format_time(Duration::from_secs(3600)), "1:00:00");
    assert_eq!(format_time(Duration::from_secs(3723)), "1:02:03");
    assert_eq!(format_remaining(Duration::ZERO), "-00:00");
    assert_eq!(format_remaining(Duration::from_secs(72)), "-01:12");
}
