/* application.rs
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

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use glib::subclass::Signal;
use gtk::{gdk, gio, glib};
use vinheta::audio::{self, AudioEngine, Config, Device, Event, PlayOptions, PlaybackId};
use vinheta::devices::{self, Entry};
use vinheta::editors;
use vinheta::library;
use vinheta::pads::{self, PadSettings, PadStore, Trigger, TriggerMode};

use crate::config::VERSION;
use crate::sound::Sound;
use crate::ui::call_guide_dialog::CallGuideDialog;
use crate::ui::preferences_dialog::PreferencesDialog;
use crate::{VinhetaWindow, APP_ID};

/// How long a stopped sound fades out when "Fade Out on Stop" is on.
const FADE_OUT: Duration = Duration::from_millis(300);
/// How often the times of the playing pads are updated.
const POSITION_INTERVAL: Duration = Duration::from_millis(100);
/// Changes of the pad settings are written at most this often.
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// An engine that ran this long before it failed starts the retries from
/// the shortest delay again.
const HEALTHY_RUN: Duration = Duration::from_secs(10);

/// The two selectors: the settings key of each, and its list. `CallApp`
/// is the app the sounds are sent to, among the ones that are recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceKind {
    CallApp,
    Output,
}

impl DeviceKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::CallApp => "call-target",
            Self::Output => "monitor-output",
        }
    }
}

/// Why audio is unavailable, as the banner tells it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioFailure {
    /// PipeWire could not be reached when the engine started.
    Unreachable,
    ConnectionLost,
    Other,
}

impl From<&audio::Error> for AudioFailure {
    fn from(error: &audio::Error) -> Self {
        match error {
            audio::Error::Unreachable(_) => Self::Unreachable,
            audio::Error::ConnectionLost(_) => Self::ConnectionLost,
            _ => Self::Other,
        }
    }
}

/// Something the user is told with a toast. It waits for the window when
/// there is none yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub message: String,
    /// Stays until it is dismissed, ahead of the other toasts.
    pub sticky: bool,
}

/// A device name short enough for a toast, which does not wrap.
fn short_name(name: &str) -> String {
    const LIMIT: usize = 32;
    if name.chars().count() <= LIMIT {
        return name.to_owned();
    }
    let cut: String = name.chars().take(LIMIT - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The engine takes `None` for the system default and for every app, the
/// settings store "".
fn device_setting(settings: &gio::Settings, key: &str) -> Option<String> {
    Some(settings.string(key).to_string()).filter(|name| !name.is_empty())
}

fn fade_out(settings: &gio::Settings) -> Duration {
    if settings.boolean("fade-out-on-stop") {
        FADE_OUT
    } else {
        Duration::ZERO
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct VinhetaApplication {
        ///  when the engine could not start or lost PipeWire.
        pub engine: RefCell<Option<AudioEngine>>,
        pub playing: RefCell<HashMap<PlaybackId, Sound>>,
        pub audio_error: Cell<Option<AudioFailure>>,
        pub engine_started: Cell<Option<Instant>>,
        /// Only exists while an automatic start of the engine waits.
        pub retry_timer: RefCell<Option<glib::SourceId>>,
        /// The failed starts in a row, for the delay of the next one.
        pub retry_attempt: Cell<u32>,
        /// Counts the engines started, so that what an old one still reports
        /// is ignored.
        pub generation: Cell<u32>,
        /// False until the engine in use listed its devices.
        pub devices_known: Cell<bool>,
        /// Whether the chosen output is missing.
        pub output_missing: Cell<bool>,
        pub notices: RefCell<Vec<Notice>>,
        pub call_guide: glib::WeakRef<CallGuideDialog>,
        /// Set when a damaged pad file could not be set aside: nothing is
        /// saved, so that it is never overwritten.
        pub pads_read_only: Cell<bool>,
        pub save_failed: Cell<bool>,
        pub settings: OnceCell<gio::Settings>,
        /// The apps that are recording a microphone.
        pub call_apps: RefCell<Vec<Device>>,
        pub outputs: RefCell<Vec<Device>>,
        /// Descriptions of the devices and apps seen in this run, by name,
        /// to keep naming a chosen one after it is gone.
        pub descriptions: RefCell<HashMap<String, String>>,
        pub preferences: glib::WeakRef<PreferencesDialog>,
        /// What the user set for each pad, stored in `pads_file`.
        pub pads: RefCell<PadStore>,
        pub pads_file: OnceCell<PathBuf>,
        /// Set while a change of `pads` waits to be written.
        pub save_timer: RefCell<Option<glib::SourceId>>,
        /// Only exists while a sound plays.
        pub position_timer: RefCell<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VinhetaApplication {
        const NAME: &'static str = "VinhetaApplication";
        type Type = super::VinhetaApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for VinhetaApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_gactions();
            obj.set_accels_for_action("app.quit", &["<control>q"]);
            obj.set_accels_for_action("app.preferences", &["<control>comma"]);
            obj.set_accels_for_action("win.search-mode", &["<control>f"]);
            obj.set_accels_for_action("win.move-folder-left", &["<control><shift>Page_Up"]);
            obj.set_accels_for_action("win.move-folder-right", &["<control><shift>Page_Down"]);
            obj.set_accels_for_action("app.stop-all", &["<control><shift>s"]);
            obj.set_accels_for_action("app.send-sounds-to-call", &["<control><shift>l"]);
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            // Emitted when the device lists or the audio availability change.
            SIGNALS.get_or_init(|| vec![Signal::builder("devices-changed").build()])
        }
    }

    impl ApplicationImpl for VinhetaApplication {
        fn startup(&self) {
            self.parent_startup();
            self.obj().load_pads();
            self.obj().setup_settings();
            self.obj().start_audio();
        }

        // Dropping the engine is what removes its node and its links.
        fn shutdown(&self) {
            if let Some(timer) = self.save_timer.take() {
                timer.remove();
                self.obj().save_pads();
            }
            if let Some(timer) = self.position_timer.take() {
                timer.remove();
            }
            if let Some(timer) = self.retry_timer.take() {
                timer.remove();
            }
            self.playing.borrow_mut().clear();
            self.engine.take();
            self.parent_shutdown();
        }

        // We connect to the activate callback to create a window when the application
        // has been launched. Additionally, this callback notifies us when the user
        // tries to launch a "second instance" of the application. When they try
        // to do that, we'll just present any existing window.
        fn activate(&self) {
            let application = self.obj();
            // Get the current window or create one if necessary
            let window = application.active_window().unwrap_or_else(|| {
                let window = VinhetaWindow::new(&*application);
                window.set_audio_error(self.audio_error.get());
                window.upcast()
            });

            // Ask the window manager/compositor to present the window
            window.present();
            application.show_notices();
            application.first_run();
        }
    }

    impl GtkApplicationImpl for VinhetaApplication {}
    impl AdwApplicationImpl for VinhetaApplication {}
}

glib::wrapper! {
    pub struct VinhetaApplication(ObjectSubclass<imp::VinhetaApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl VinhetaApplication {
    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .property("resource-base-path", "/io/github/wilfison/Vinheta")
            .build()
    }

    fn setup_gactions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        // The parameter is the absolute path of a sound of the library.
        let sound_action = |name: &str, activate: fn(&Self, &Sound)| {
            gio::ActionEntry::builder(name)
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(move |app: &Self, _, path| {
                    let path = path.and_then(|path| path.str());
                    if let Some(sound) = path.and_then(|path| app.find_sound(path)) {
                        activate(app, &sound);
                    }
                })
                .build()
        };
        let toggle_sound_action = sound_action("toggle-sound", |app, sound| {
            let mode = app.trigger_mode();
            app.trigger_sound(sound, pads::trigger(mode, sound.playing()));
        });
        // What the search uses: it never stops a sound.
        let play_sound_action = sound_action("play-sound", |app, sound| {
            if let Some(trigger) = pads::play(app.trigger_mode(), sound.playing()) {
                app.trigger_sound(sound, trigger);
            }
        });
        let stop_sound_action = sound_action("stop-sound", Self::stop_sound);
        let toggle_favorite_action = sound_action("toggle-favorite", |app, sound| {
            let mut settings = sound.settings();
            settings.favorite = !settings.favorite;
            app.update_sound(sound, settings);
        });
        let trash_sound_action = sound_action("trash-sound", Self::trash_sound);
        let open_in_editor_action = sound_action("open-in-editor", Self::open_in_editor);
        let toggle_loop_action = sound_action("toggle-loop", |app, sound| {
            let mut settings = sound.settings();
            settings.looping = !settings.looping;
            app.update_sound(sound, settings);
        });
        let reset_sound_action = sound_action("reset-sound", |app, sound| {
            app.update_sound(sound, PadSettings::default());
        });
        // The parameter is the path of a sound of the library and its key,
        // or an empty text for no key.
        let set_shortcut_action = gio::ActionEntry::builder("set-shortcut")
            .parameter_type(Some(glib::VariantTy::new("(ss)").unwrap()))
            .activate(move |app: &Self, _, parameter| {
                let parameter = parameter.and_then(|parameter| parameter.get::<(String, String)>());
                if let Some((path, key)) = parameter {
                    app.set_shortcut(&path, &key);
                }
            })
            .build();
        // The parameter is a pad key: what a key press in the window does.
        let trigger_shortcut_action = gio::ActionEntry::builder("trigger-shortcut")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, key| {
                let mut chars = key.and_then(|key| key.str()).unwrap_or_default().chars();
                if let (Some(key), None) = (chars.next(), chars.next()) {
                    app.trigger_shortcut(key);
                }
            })
            .build();
        let stop_all_action = gio::ActionEntry::builder("stop-all")
            .activate(move |app: &Self, _, _| app.stop_all())
            .build();
        let preferences_action = gio::ActionEntry::builder("preferences")
            .activate(move |app: &Self, _, _| app.show_preferences())
            .build();
        let call_guide_action = gio::ActionEntry::builder("call-guide")
            .activate(move |app: &Self, _, _| app.show_call_guide())
            .build();
        // Only enabled while audio is unavailable.
        let retry_audio_action = gio::ActionEntry::builder("retry-audio")
            .activate(move |app: &Self, _, _| app.retry_audio())
            .build();
        self.add_action_entries([
            quit_action,
            about_action,
            preferences_action,
            call_guide_action,
            retry_audio_action,
            toggle_sound_action,
            play_sound_action,
            stop_sound_action,
            toggle_loop_action,
            toggle_favorite_action,
            trash_sound_action,
            open_in_editor_action,
            reset_sound_action,
            set_shortcut_action,
            trigger_shortcut_action,
            stop_all_action,
        ]);
        self.update_playing();
    }

    fn settings(&self) -> &gio::Settings {
        self.imp().settings.get().unwrap()
    }

    /// The settings are the source of truth of the mix: every change of a
    /// key is forwarded to the engine.
    fn setup_settings(&self) {
        let settings = gio::Settings::new(APP_ID);
        settings.connect_changed(
            None,
            glib::clone!(
                #[weak(rename_to = app)]
                self,
                move |settings, key| {
                    {
                        let engine = app.imp().engine.borrow();
                        let Some(engine) = engine.as_ref() else {
                            return;
                        };
                        match key {
                            "send-sounds-to-call" => engine.set_send_to_call(settings.boolean(key)),
                            "call-volume" => {
                                engine.set_call_volume(audio::slider_gain(settings.double(key)));
                            }
                            "monitor-volume" => {
                                engine.set_monitor_volume(audio::slider_gain(settings.double(key)));
                            }
                            "call-target" => engine.set_target(device_setting(settings, key)),
                            "monitor-output" => engine.set_monitor(device_setting(settings, key)),
                            "fade-out-on-stop" => engine.set_fade_out(fade_out(settings)),
                            _ => {}
                        }
                    }
                    if key == "call-target" || key == "monitor-output" {
                        app.update_missing_devices();
                        app.emit_by_name::<()>("devices-changed", &[]);
                    }
                }
            ),
        );
        // Stateful actions that flip a key, for the accelerators.
        self.add_action(&settings.create_action("send-sounds-to-call"));
        self.imp().settings.set(settings).unwrap();
    }

    /// Starts an engine with the current settings. Returns whether it did.
    fn start_audio(&self) -> bool {
        let imp = self.imp();
        let settings = self.settings();
        let config = Config {
            target: device_setting(settings, "call-target"),
            monitor: device_setting(settings, "monitor-output"),
            call_volume: audio::slider_gain(settings.double("call-volume")),
            monitor_volume: audio::slider_gain(settings.double("monitor-volume")),
            send_to_call: settings.boolean("send-sounds-to-call"),
            fade_out: fade_out(settings),
        };
        let generation = imp.generation.get().wrapping_add(1);
        imp.generation.set(generation);
        let started = match AudioEngine::start(config) {
            Ok((engine, events)) => {
                imp.engine.replace(Some(engine));
                imp.audio_error.set(None);
                imp.engine_started.set(Some(Instant::now()));
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = app)]
                    self,
                    async move {
                        while let Ok(event) = events.recv().await {
                            if app.imp().generation.get() != generation {
                                break;
                            }
                            app.handle_event(event);
                        }
                    }
                ));
                true
            }
            Err(error) => {
                self.set_audio_error(&error);
                false
            }
        };
        if let Some(action) = self.lookup_action("retry-audio") {
            if let Some(action) = action.downcast_ref::<gio::SimpleAction>() {
                action.set_enabled(!started);
            }
        }
        started
    }

    /// What "Try Again" and the automatic retries do. A failure leaves the
    /// banner with the new reason and schedules the next retry.
    fn retry_audio(&self) {
        if let Some(timer) = self.imp().retry_timer.take() {
            timer.remove();
        }
        if self.audio_available() || !self.start_audio() {
            return;
        }
        if let Some(window) = self.window() {
            window.set_audio_error(None);
        }
        self.emit_by_name::<()>("devices-changed", &[]);
    }

    fn handle_event(&self, event: Event) {
        match event {
            Event::PlaybackFinished { id, .. } => {
                self.playback_ended(id);
            }
            Event::Error(audio::Error::Playback { id, message, .. }) => {
                glib::g_warning!("vinheta", "playback failed: {message}");
                if let Some(sound) = self.playback_ended(id) {
                    self.toast_playback_failure(&sound);
                }
            }
            Event::DevicesChanged { outputs } => {
                self.remember(&outputs);
                self.imp().outputs.replace(outputs);
                self.imp().devices_known.set(true);
                self.update_missing_devices();
                self.emit_by_name::<()>("devices-changed", &[]);
            }
            Event::TargetsChanged(apps) => {
                self.remember(&apps);
                self.imp().call_apps.replace(apps);
                self.emit_by_name::<()>("devices-changed", &[]);
            }
            Event::Error(error @ (audio::Error::PipeWire(_) | audio::Error::ConnectionLost(_))) => {
                self.set_audio_error(&error)
            }
            Event::Error(error) => glib::g_warning!("vinheta", "{error}"),
            event => glib::g_debug!("vinheta", "{event:?}"),
        }
    }

    fn load_pads(&self) {
        let imp = self.imp();
        let file = glib::user_data_dir().join("vinheta").join("pads.json");
        let mut pruned = 0;
        match PadStore::load(&file) {
            Ok(mut pads) => {
                pruned = pads.prune_missing();
                imp.pads.replace(pads);
            }
            // The file is set aside, so that the next save does not destroy it.
            Err(error) => {
                let aside = file.with_extension("json.corrupt");
                glib::g_warning!(
                    "vinheta",
                    "{}: {error}; moving it to {}",
                    file.display(),
                    aside.display()
                );
                let message = match std::fs::rename(&file, &aside) {
                    Ok(()) => gettext(
                        "Pad settings could not be read and were reset. The old file was kept as “pads.json.corrupt”.",
                    ),
                    Err(error) => {
                        glib::g_warning!("vinheta", "could not move it: {error}");
                        imp.pads_read_only.set(true);
                        gettext("Pad settings could not be read. Changes to pads will not be saved.")
                    }
                };
                self.notify(&message, true);
            }
        }
        imp.pads_file.set(file).unwrap();
        if pruned > 0 {
            glib::g_debug!("vinheta", "removed the settings of {pruned} missing files");
            self.save_pads();
        }
    }

    fn save_pads(&self) {
        let imp = self.imp();
        let Some(file) = imp.pads_file.get() else {
            return;
        };
        if imp.pads_read_only.get() {
            return;
        }
        if let Err(error) = imp.pads.borrow().save(file) {
            glib::g_warning!("vinheta", "could not save {}: {error}", file.display());
            // Once per session, not once per attempt.
            if !imp.save_failed.replace(true) {
                self.notify(&gettext("Pad settings could not be saved"), false);
            }
        }
    }

    /// Tells the user something with a toast, now or once there is a window.
    fn notify(&self, message: &str, sticky: bool) {
        self.imp().notices.borrow_mut().push(Notice {
            message: message.to_owned(),
            sticky,
        });
        self.show_notices();
    }

    fn show_notices(&self) {
        let Some(window) = self.window() else {
            return;
        };
        let notices = std::mem::take(&mut *self.imp().notices.borrow_mut());
        for notice in &notices {
            window.notice(notice);
        }
    }

    fn schedule_save(&self) {
        let mut timer = self.imp().save_timer.borrow_mut();
        if timer.is_some() {
            return;
        }
        *timer = Some(glib::timeout_add_local_once(
            SAVE_DELAY,
            glib::clone!(
                #[weak(rename_to = app)]
                self,
                move || {
                    app.imp().save_timer.take();
                    app.save_pads();
                }
            ),
        ));
    }

    /// What the user set for the pad of a file.
    pub fn pad_settings(&self, path: &str) -> PadSettings {
        self.imp().pads.borrow().get(path)
    }

    /// The one way to change the settings of a sound: it reaches the pad,
    /// the file, and the playback of that sound if there is one.
    pub fn update_sound(&self, sound: &Sound, settings: PadSettings) {
        let imp = self.imp();
        let path = sound.path();
        let (settings, taken) = {
            let mut pads = imp.pads.borrow_mut();
            pads.set(&path, settings);
            let settings = pads.get(&path);
            // A key belongs to one pad.
            let taken = settings.shortcut.map(|key| pads.take_shortcut(key, &path));
            (settings, taken.unwrap_or_default())
        };
        for other in &taken {
            self.schedule_save();
            if let Some(other) = self.find_sound(other) {
                if let (Some(window), Some(key)) = (self.window(), settings.shortcut) {
                    window.shortcut_taken(sound, key, &other);
                }
                other.set_settings(&self.pad_settings(&other.path()));
            }
        }
        let old = sound.settings();
        if settings == old {
            return;
        }
        if let (Some(engine), Some(id)) = (imp.engine.borrow().as_ref(), self.playback_of(sound)) {
            engine.set_playback_volume(id, audio::slider_gain(settings.volume));
            engine.set_playback_loop(id, settings.looping);
        }
        sound.set_settings(&settings);
        self.schedule_save();
        // The sorted and filtered views do not watch the sounds themselves.
        if let Some(window) = self.window() {
            window.sound_changed(old.name != settings.name, old.favorite != settings.favorite);
        }
    }

    /// Gives a sound of the library its key, or none for an empty text.
    /// Anything that is not a letter or a digit is ignored.
    fn set_shortcut(&self, path: &str, key: &str) {
        let mut chars = key.chars();
        let key = match (chars.next(), chars.next()) {
            (None, _) => None,
            (Some(key), None) if pads::shortcut_key(key).is_some() => Some(key),
            _ => return,
        };
        if let Some(sound) = self.find_sound(path) {
            let mut settings = sound.settings();
            settings.shortcut = key;
            self.update_sound(&sound, settings);
        }
    }

    /// Does what a click on the pad with this key does. Returns whether a
    /// pad of the library has the key.
    pub fn trigger_shortcut(&self, key: char) -> bool {
        let owner = {
            let pads = self.imp().pads.borrow();
            pads.shortcut_owner(key).map(str::to_owned)
        };
        let Some(sound) = owner.and_then(|path| self.find_sound(&path)) else {
            return false;
        };
        self.trigger_sound(&sound, pads::trigger(self.trigger_mode(), sound.playing()));
        true
    }

    /// Moves the settings of every file of a folder to another folder.
    pub fn move_folder_settings(&self, from: &str, to: &str) {
        if self.imp().pads.borrow_mut().move_folder(from, to) > 0 {
            self.schedule_save();
        }
    }

    /// Moves the settings of a file that was renamed or moved.
    pub fn move_pad_settings(&self, from: &str, to: &str) {
        if self.imp().pads.borrow_mut().rename(from, to) {
            self.schedule_save();
        }
    }

    /// For a sound whose file went away: it stops and leaves the dialog that
    /// edits it. Its settings are kept, the file may come back.
    pub fn forget_sound(&self, sound: &Sound) {
        self.stop_sound(sound);
        if let Some(window) = self.window() {
            window.sound_gone(sound);
        }
    }

    /// The folder that receives the copies of loose files.
    pub fn sounds_folder(&self) -> PathBuf {
        let settings = self.imp().settings.get();
        let chosen = settings.map(|settings| settings.string("sounds-folder"));
        match chosen.filter(|folder| !folder.is_empty()) {
            Some(folder) => PathBuf::from(folder.as_str()),
            None => glib::user_data_dir().join("vinheta").join("sounds"),
        }
    }

    /// Whether the file is a copy made by the app, which it may trash.
    pub fn is_imported(&self, path: &str) -> bool {
        Path::new(path).parent() == Some(self.sounds_folder().as_path())
    }

    fn trash_sound(&self, sound: &Sound) {
        let path = sound.path();
        if !self.is_imported(&path) {
            return;
        }
        self.forget_sound(sound);
        let message = match gio::File::for_path(&path).trash(gio::Cancellable::NONE) {
            // Translators: {} is the name of a sound.
            Ok(()) => gettext("Moved “{}” to the trash"),
            Err(error) => {
                glib::g_warning!("vinheta", "could not trash {path}: {error}");
                // Translators: {} is the name of a sound.
                gettext("Could not move “{}” to the trash")
            }
        };
        if let Some(window) = self.window() {
            window.toast(&message.replace("{}", &sound.display_name()));
        }
    }

    /// The installed apps that open the formats of the library, each with
    /// what launches it.
    pub fn audio_apps(&self) -> Vec<(editors::App, gio::AppInfo)> {
        let mut apps: Vec<(editors::App, gio::AppInfo)> = Vec::new();
        for extension in library::EXTENSIONS {
            let (content_type, _) = gio::content_type_guess(Some(format!("a.{extension}")), &[]);
            for info in gio::AppInfo::all_for_type(&content_type) {
                let Some(id) = info.id() else { continue };
                if id == format!("{APP_ID}.desktop") || apps.iter().any(|(app, _)| app.id == id) {
                    continue;
                }
                let desktop = info.downcast_ref::<gio::DesktopAppInfo>();
                let categories = desktop.and_then(|desktop| desktop.categories());
                let categories = categories
                    .iter()
                    .flat_map(|categories| categories.split(';'))
                    .filter(|category| !category.is_empty())
                    .map(str::to_owned)
                    .collect();
                let app = editors::App {
                    id: id.into(),
                    name: info.name().into(),
                    categories,
                };
                apps.push((app, info));
            }
        }
        apps
    }

    /// The app that opens a sound for editing: the `audio-editor` key, or
    /// the first audio editor that is installed.
    pub fn audio_editor(&self) -> Option<gio::AppInfo> {
        let apps = self.audio_apps();
        let (list, mut infos): (Vec<_>, Vec<_>) = apps.into_iter().unzip();
        let chosen = self.settings().string("audio-editor");
        let picked = editors::pick(&list, &chosen)?;
        let position = list.iter().position(|app| app == picked)?;
        Some(infos.swap_remove(position))
    }

    fn open_in_editor(&self, sound: &Sound) {
        let path = sound.path();
        let context = gdk::Display::default().map(|display| display.app_launch_context());
        let launched = match self.audio_editor() {
            Some(editor) => editor
                .launch(&[gio::File::for_path(&path)], context.as_ref())
                .map_err(|error| error.to_string()),
            None => Err("no audio editor is installed".to_owned()),
        };
        if let Err(error) = launched {
            glib::g_warning!("vinheta", "could not open {path} in an editor: {error}");
            if let Some(window) = self.window() {
                window.toast(&gettext("Could not open the audio editor"));
            }
        }
    }

    /// The file of a sound changed: the order of the pads may depend on it.
    pub fn sounds_modified(&self) {
        if let Some(window) = self.window() {
            window.sound_changed(true, false);
        }
    }

    fn trigger_mode(&self) -> TriggerMode {
        TriggerMode::from_name(&self.settings().string("trigger-mode"))
    }

    pub fn find_sound(&self, path: &str) -> Option<Sound> {
        self.window().and_then(|window| window.find_sound(path))
    }

    fn playback_of(&self, sound: &Sound) -> Option<PlaybackId> {
        let playing = self.imp().playing.borrow();
        let found = playing.iter().find(|(_, playing)| *playing == sound);
        found.map(|(id, _)| *id)
    }

    fn trigger_sound(&self, sound: &Sound, trigger: Trigger) {
        let imp = self.imp();
        let engine = imp.engine.borrow();
        let Some(engine) = engine.as_ref() else {
            return;
        };
        match (trigger, self.playback_of(sound)) {
            (Trigger::Stop, Some(id)) => {
                engine.stop(id);
                self.playback_ended(id);
            }
            (Trigger::Restart, Some(id)) => {
                engine.restart(id);
                sound.set_position(None);
            }
            (Trigger::StartAlone, _) => {
                let others: Vec<_> = imp.playing.borrow().keys().copied().collect();
                for id in others {
                    engine.stop(id);
                    self.playback_ended(id);
                }
                self.start_sound(engine, sound);
            }
            _ => self.start_sound(engine, sound),
        }
    }

    fn start_sound(&self, engine: &AudioEngine, sound: &Sound) {
        let options = PlayOptions {
            volume: audio::slider_gain(sound.volume()),
            looping: sound.looping(),
        };
        let started = Instant::now();
        let result = engine.play(sound.path(), options);
        glib::g_debug!(
            "vinheta",
            "starting {} took {:?}",
            sound.path(),
            started.elapsed()
        );
        match result {
            Ok(id) => {
                self.imp().playing.borrow_mut().insert(id, sound.clone());
                sound.set_playing(true);
                self.update_playing();
            }
            Err(error) => {
                glib::g_warning!("vinheta", "{error}");
                self.toast_playback_failure(sound);
            }
        }
    }

    fn stop_sound(&self, sound: &Sound) {
        let Some(id) = self.playback_of(sound) else {
            return;
        };
        if let Some(engine) = self.imp().engine.borrow().as_ref() {
            engine.stop(id);
        }
        self.playback_ended(id);
    }

    fn stop_all(&self) {
        if let Some(engine) = self.imp().engine.borrow().as_ref() {
            engine.stop_all();
        }
        self.forget_playbacks();
    }

    /// Returns the sound of a playback that is over, if it was still known.
    fn playback_ended(&self, id: PlaybackId) -> Option<Sound> {
        let sound = self.imp().playing.borrow_mut().remove(&id)?;
        sound.set_playing(false);
        sound.set_position(None);
        self.update_playing();
        Some(sound)
    }

    fn forget_playbacks(&self) {
        let playing = std::mem::take(&mut *self.imp().playing.borrow_mut());
        for sound in playing.values() {
            sound.set_playing(false);
            sound.set_position(None);
        }
        self.update_playing();
    }

    fn update_playing(&self) {
        let imp = self.imp();
        let count = imp.playing.borrow().len();
        if let Some(action) = self.lookup_action("stop-all") {
            if let Some(action) = action.downcast_ref::<gio::SimpleAction>() {
                action.set_enabled(count > 0);
            }
        }
        if let Some(window) = self.window() {
            window.set_playing_count(count);
        }

        // The timer only exists while a sound plays.
        let mut timer = imp.position_timer.borrow_mut();
        if count == 0 {
            if let Some(timer) = timer.take() {
                timer.remove();
            }
        } else if timer.is_none() {
            *timer = Some(glib::timeout_add_local(
                POSITION_INTERVAL,
                glib::clone!(
                    #[weak(rename_to = app)]
                    self,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        app.update_positions();
                        glib::ControlFlow::Continue
                    }
                ),
            ));
        }
    }

    fn update_positions(&self) {
        let imp = self.imp();
        let engine = imp.engine.borrow();
        let Some(engine) = engine.as_ref() else {
            return;
        };
        // Notifying may reach back into the application.
        let playing: Vec<_> = imp
            .playing
            .borrow()
            .iter()
            .map(|(id, sound)| (*id, sound.clone()))
            .collect();
        let millis = |time: Duration| i64::try_from(time.as_millis()).unwrap_or(i64::MAX);
        for (id, sound) in playing {
            let position = engine.position(id);
            sound.set_position(
                position.map(|position| (millis(position.elapsed), position.duration.map(millis))),
            );
        }
    }

    fn set_audio_error(&self, error: &audio::Error) {
        let imp = self.imp();
        let started = imp.engine_started.take();
        if started.is_some_and(|started| started.elapsed() >= HEALTHY_RUN) {
            imp.retry_attempt.set(0);
        }
        // Once per outage, not once per failed retry.
        if started.is_some() || imp.retry_attempt.get() == 0 {
            glib::g_warning!("vinheta", "audio is unavailable: {error}");
        } else {
            glib::g_debug!("vinheta", "audio is still unavailable: {error}");
        }
        let failure = AudioFailure::from(error);
        if let Some(window) = self.window() {
            window.set_audio_error(Some(failure));
        }
        imp.audio_error.set(Some(failure));
        imp.engine.take();
        imp.devices_known.set(false);
        imp.output_missing.set(false);
        if let Some(action) = self.lookup_action("retry-audio") {
            if let Some(action) = action.downcast_ref::<gio::SimpleAction>() {
                action.set_enabled(true);
            }
        }
        self.forget_playbacks();
        imp.call_apps.take();
        imp.outputs.take();
        self.emit_by_name::<()>("devices-changed", &[]);
        self.schedule_retry();
    }

    fn schedule_retry(&self) {
        let imp = self.imp();
        let mut timer = imp.retry_timer.borrow_mut();
        if timer.is_some() {
            return;
        }
        let attempt = imp.retry_attempt.get();
        imp.retry_attempt.set(attempt.saturating_add(1));
        let delay = audio::retry_delay(attempt);
        glib::g_debug!("vinheta", "retrying audio in {delay:?}");
        *timer = Some(glib::timeout_add_local_once(
            delay,
            glib::clone!(
                #[weak(rename_to = app)]
                self,
                move || {
                    app.imp().retry_timer.take();
                    app.retry_audio();
                }
            ),
        ));
    }

    fn remember(&self, devices: &[Device]) {
        self.imp().descriptions.borrow_mut().extend(
            devices
                .iter()
                .map(|device| (device.name.clone(), device.description.clone())),
        );
    }

    /// Tells the user when the chosen output goes from present to missing:
    /// once per change, and nothing when it returns. A chosen app that is
    /// not recording is the usual state, so nothing is said about it.
    fn update_missing_devices(&self) {
        let imp = self.imp();
        if !imp.devices_known.get() {
            return;
        }
        let chosen = self.settings().string(DeviceKind::Output.key());
        let missing = devices::is_missing(&imp.outputs.borrow(), &chosen);
        if missing && !imp.output_missing.replace(missing) {
            let descriptions = imp.descriptions.borrow();
            let name = descriptions
                .get(chosen.as_str())
                .map_or(&*chosen, String::as_str);
            // Translators: {} is the name of an audio output.
            let message = gettext("Output “{}” is not connected. Using the system default.");
            self.notify(&message.replace("{}", &short_name(name)), false);
        }
        imp.output_missing.set(missing);
    }

    /// Whether the chosen output is not connected, or the chosen app is not
    /// recording. Never while audio is unavailable: the banner already
    /// says so.
    pub fn device_missing(&self, kind: DeviceKind) -> bool {
        let imp = self.imp();
        self.audio_available()
            && match kind {
                DeviceKind::CallApp => {
                    let chosen = self.settings().string(kind.key());
                    devices::is_missing(&imp.call_apps.borrow(), &chosen)
                }
                DeviceKind::Output => imp.output_missing.get(),
            }
    }

    /// Whether some app is recording a microphone, so that the sounds have
    /// somewhere to go.
    pub fn call_app_recording(&self) -> bool {
        !self.imp().call_apps.borrow().is_empty()
    }

    pub fn audio_available(&self) -> bool {
        self.imp().engine.borrow().is_some()
    }

    /// The entries of a selector and the selected position. Without audio
    /// there is only the first one: the system default, or every app.
    pub fn device_entries(&self, kind: DeviceKind) -> (Vec<Entry>, usize) {
        let imp = self.imp();
        if !self.audio_available() {
            return devices::selector_entries(&[], "", None);
        }
        let chosen = self.settings().string(kind.key());
        let devices = match kind {
            DeviceKind::CallApp => imp.call_apps.borrow(),
            DeviceKind::Output => imp.outputs.borrow(),
        };
        let descriptions = imp.descriptions.borrow();
        let last_description = descriptions.get(chosen.as_str()).map(String::as_str);
        devices::selector_entries(&devices, &chosen, last_description)
    }

    fn show_preferences(&self) {
        let imp = self.imp();
        if imp.preferences.upgrade().is_some() {
            return;
        }
        let dialog = PreferencesDialog::new(self);
        imp.preferences.set(Some(&dialog));
        dialog.present(self.active_window().as_ref());
    }

    fn show_call_guide(&self) {
        let imp = self.imp();
        if imp.call_guide.upgrade().is_some() {
            return;
        }
        let dialog = CallGuideDialog::new();
        // However it is closed, it was shown.
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = app)]
            self,
            move |_| {
                if let Err(error) = app.settings().set_boolean("call-guide-shown", true) {
                    glib::g_warning!(
                        "vinheta",
                        "could not save that the guide was shown: {error}"
                    );
                }
            }
        ));
        imp.call_guide.set(Some(&dialog));
        dialog.present(self.active_window().as_ref());
    }

    /// The guide opens by itself on the first run. Not without audio: the
    /// banner comes first, and the guide waits for a later launch.
    fn first_run(&self) {
        if self.audio_available() && !self.settings().boolean("call-guide-shown") {
            self.show_call_guide();
        }
    }

    fn toast_playback_failure(&self, sound: &Sound) {
        if let Some(window) = self.window() {
            // Translators: {} is the name of a sound.
            window.toast(&gettext("Could not play “{}”").replace("{}", &sound.display_name()));
        }
    }

    fn window(&self) -> Option<VinhetaWindow> {
        self.windows()
            .into_iter()
            .find_map(|window| window.downcast().ok())
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let about = adw::AboutDialog::builder()
            .application_name("Vinheta")
            .application_icon("io.github.wilfison.Vinheta")
            .developer_name("wilfison")
            .version(VERSION)
            .comments(gettext("Play sounds into your calls"))
            .website("https://github.com/wilfison/Vinheta")
            .issue_url("https://github.com/wilfison/Vinheta/issues")
            .license_type(gtk::License::Gpl30)
            .developers(vec!["wilfison"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(gettext("translator-credits"))
            .copyright("© 2026 wilfison")
            .build();

        about.present(Some(&window));
    }
}
