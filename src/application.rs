/* application.rs
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

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use glib::subclass::Signal;
use gtk::{gio, glib};
use vinheta::audio::{self, AudioEngine, Config, Device, Event, PlayOptions, PlaybackId};
use vinheta::devices::{self, Entry};
use vinheta::pads::{self, PadSettings, PadStore, Trigger, TriggerMode};

use crate::config::VERSION;
use crate::sound::Sound;
use crate::ui::preferences_dialog::PreferencesDialog;
use crate::{VinhetaWindow, APP_ID};

/// How long a stopped sound fades out when "Fade Out on Stop" is on.
const FADE_OUT: Duration = Duration::from_millis(300);
/// How often the times of the playing pads are updated.
const POSITION_INTERVAL: Duration = Duration::from_millis(100);
/// Changes of the pad settings are written at most this often.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// The two device selectors: the settings key of each, and its list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceKind {
    Microphone,
    Output,
}

impl DeviceKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Output => "monitor-output",
        }
    }
}

/// The engine takes `None` for the system default, the settings store "".
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
        pub audio_error: RefCell<Option<String>>,
        pub settings: OnceCell<gio::Settings>,
        pub microphones: RefCell<Vec<Device>>,
        pub outputs: RefCell<Vec<Device>>,
        /// Descriptions of the devices seen in this run, by node name, to
        /// keep naming a chosen device after it is removed.
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
            self.obj().start_audio();
        }

        // Dropping the engine is what removes the virtual microphone.
        fn shutdown(&self) {
            if let Some(timer) = self.save_timer.take() {
                timer.remove();
                self.obj().save_pads();
            }
            if let Some(timer) = self.position_timer.take() {
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
                window.set_audio_error(self.audio_error.borrow().as_deref());
                window.upcast()
            });

            // Ask the window manager/compositor to present the window
            window.present();
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
        let toggle_sound_action = gio::ActionEntry::builder("toggle-sound")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, path| {
                if let Some(path) = path.and_then(|path| path.str()) {
                    app.toggle_sound(path);
                }
            })
            .build();
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
        let stop_sound_action = sound_action("stop-sound", Self::stop_sound);
        let toggle_loop_action = sound_action("toggle-loop", |app, sound| {
            let mut settings = sound.settings();
            settings.looping = !settings.looping;
            app.update_sound(sound, settings);
        });
        let reset_sound_action = sound_action("reset-sound", |app, sound| {
            app.update_sound(sound, PadSettings::default());
        });
        let stop_all_action = gio::ActionEntry::builder("stop-all")
            .activate(move |app: &Self, _, _| app.stop_all())
            .build();
        let preferences_action = gio::ActionEntry::builder("preferences")
            .activate(move |app: &Self, _, _| app.show_preferences())
            .build();
        self.add_action_entries([
            quit_action,
            about_action,
            preferences_action,
            toggle_sound_action,
            stop_sound_action,
            toggle_loop_action,
            reset_sound_action,
            stop_all_action,
        ]);
        self.update_playing();
    }

    fn start_audio(&self) {
        let settings = gio::Settings::new(APP_ID);
        let config = Config {
            mic: device_setting(&settings, "microphone"),
            monitor: device_setting(&settings, "monitor-output"),
            call_volume: audio::slider_gain(settings.double("call-volume")),
            monitor_volume: audio::slider_gain(settings.double("monitor-volume")),
            send_to_call: settings.boolean("send-sounds-to-call"),
            include_voice: settings.boolean("include-my-voice"),
            fade_out: fade_out(&settings),
        };
        match AudioEngine::start(config) {
            Ok((engine, events)) => {
                self.imp().engine.replace(Some(engine));
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = app)]
                    self,
                    async move {
                        while let Ok(event) = events.recv().await {
                            app.handle_event(event);
                        }
                    }
                ));
            }
            Err(error) => self.set_audio_error(&error),
        }

        settings.connect_changed(
            None,
            glib::clone!(
                #[weak(rename_to = app)]
                self,
                move |settings, key| {
                    let engine = app.imp().engine.borrow();
                    let Some(engine) = engine.as_ref() else {
                        return;
                    };
                    match key {
                        "send-sounds-to-call" => engine.set_send_to_call(settings.boolean(key)),
                        "include-my-voice" => engine.set_include_voice(settings.boolean(key)),
                        "call-volume" => {
                            engine.set_call_volume(audio::slider_gain(settings.double(key)));
                        }
                        "monitor-volume" => {
                            engine.set_monitor_volume(audio::slider_gain(settings.double(key)));
                        }
                        "microphone" => engine.set_microphone(device_setting(settings, key)),
                        "monitor-output" => engine.set_monitor(device_setting(settings, key)),
                        "fade-out-on-stop" => engine.set_fade_out(fade_out(settings)),
                        _ => {}
                    }
                }
            ),
        );
        self.imp().settings.set(settings).unwrap();
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
            Event::DevicesChanged {
                microphones,
                outputs,
            } => self.set_devices(microphones, outputs),
            Event::Error(error @ audio::Error::PipeWire(_)) => self.set_audio_error(&error),
            Event::Error(error) => glib::g_warning!("vinheta", "{error}"),
            event => glib::g_debug!("vinheta", "{event:?}"),
        }
    }

    fn load_pads(&self) {
        let file = glib::user_data_dir().join("vinheta").join("pads.json");
        match PadStore::load(&file) {
            Ok(pads) => {
                self.imp().pads.replace(pads);
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
                if let Err(error) = std::fs::rename(&file, &aside) {
                    glib::g_warning!("vinheta", "could not move it: {error}");
                }
            }
        }
        self.imp().pads_file.set(file).unwrap();
    }

    fn save_pads(&self) {
        let imp = self.imp();
        let Some(file) = imp.pads_file.get() else {
            return;
        };
        if let Err(error) = imp.pads.borrow().save(file) {
            glib::g_warning!("vinheta", "could not save {}: {error}", file.display());
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
        let settings = {
            let mut pads = imp.pads.borrow_mut();
            pads.set(&path, settings);
            pads.get(&path)
        };
        if settings == sound.settings() {
            return;
        }
        if let (Some(engine), Some(id)) = (imp.engine.borrow().as_ref(), self.playback_of(sound)) {
            engine.set_playback_volume(id, audio::slider_gain(settings.volume));
            engine.set_playback_loop(id, settings.looping);
        }
        sound.set_settings(&settings);
        self.schedule_save();
    }

    pub fn find_sound(&self, path: &str) -> Option<Sound> {
        self.window().and_then(|window| window.find_sound(path))
    }

    fn playback_of(&self, sound: &Sound) -> Option<PlaybackId> {
        let playing = self.imp().playing.borrow();
        let found = playing.iter().find(|(_, playing)| *playing == sound);
        found.map(|(id, _)| *id)
    }

    fn toggle_sound(&self, path: &str) {
        let imp = self.imp();
        let Some(sound) = self.find_sound(path) else {
            return;
        };
        let engine = imp.engine.borrow();
        let Some(engine) = engine.as_ref() else {
            return;
        };

        let mode = TriggerMode::from_name(&imp.settings.get().unwrap().string("trigger-mode"));
        let playing = self.playback_of(&sound);
        match (pads::trigger(mode, playing.is_some()), playing) {
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
                self.start_sound(engine, &sound);
            }
            _ => self.start_sound(engine, &sound),
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
        glib::g_warning!("vinheta", "audio is unavailable: {error}");
        let message = error.to_string();
        if let Some(window) = self.window() {
            window.set_audio_error(Some(&message));
        }
        self.imp().audio_error.replace(Some(message));
        self.imp().engine.take();
        self.forget_playbacks();
        self.set_devices(Vec::new(), Vec::new());
    }

    fn set_devices(&self, microphones: Vec<Device>, outputs: Vec<Device>) {
        let imp = self.imp();
        imp.descriptions.borrow_mut().extend(
            microphones
                .iter()
                .chain(&outputs)
                .map(|device| (device.name.clone(), device.description.clone())),
        );
        imp.microphones.replace(microphones);
        imp.outputs.replace(outputs);
        self.emit_by_name::<()>("devices-changed", &[]);
    }

    pub fn audio_available(&self) -> bool {
        self.imp().engine.borrow().is_some()
    }

    /// The entries of a device selector and the selected position. Without
    /// audio there is only the system default.
    pub fn device_entries(&self, kind: DeviceKind) -> (Vec<Entry>, usize) {
        let imp = self.imp();
        if !self.audio_available() {
            return devices::selector_entries(&[], "", None);
        }
        let chosen = imp.settings.get().unwrap().string(kind.key());
        let devices = match kind {
            DeviceKind::Microphone => imp.microphones.borrow(),
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
            .developer_name("Will")
            .version(VERSION)
            .developers(vec!["Will"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(gettext("translator-credits"))
            .copyright("© 2026 Will")
            .build();

        about.present(Some(&window));
    }
}
