/* sound.rs
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
use std::path::Path;
use std::time::{Duration, SystemTime};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use vinheta::pads::{PadColor, PadSettings};

mod imp {
    use super::*;

    #[derive(glib::Properties)]
    #[properties(wrapper_type = super::Sound)]
    pub struct Sound {
        #[property(get, construct_only)]
        path: OnceCell<String>,
        /// The name from the file, made readable (`library::humanize`).
        #[property(get, construct_only)]
        name: OnceCell<String>,
        #[property(get, set)]
        playing: Cell<bool>,
        /// The custom name, or the name from the file.
        #[property(get = Self::display_name, name = "display-name", type = String)]
        pub custom_name: RefCell<Option<String>>,
        /// The palette name, or empty.
        #[property(get)]
        pub color: RefCell<String>,
        /// A slider position, 0 to 1.
        #[property(get)]
        pub volume: Cell<f64>,
        #[property(get)]
        pub looping: Cell<bool>,
        #[property(get)]
        pub favorite: Cell<bool>,
        /// The key that triggers the pad, or empty.
        #[property(get)]
        pub shortcut: RefCell<String>,
        /// When the file was last modified, in seconds since the epoch.
        #[property(get, set, construct)]
        modified: Cell<i64>,
        /// Milliseconds into the playback, -1 while not known.
        #[property(get)]
        pub elapsed: Cell<i64>,
        /// Milliseconds, -1 while not known.
        #[property(get)]
        pub duration: Cell<i64>,
    }

    impl Default for Sound {
        fn default() -> Self {
            Self {
                path: OnceCell::new(),
                name: OnceCell::new(),
                playing: Cell::new(false),
                custom_name: RefCell::new(None),
                color: RefCell::default(),
                volume: Cell::new(1.0),
                looping: Cell::new(false),
                favorite: Cell::new(false),
                shortcut: RefCell::default(),
                modified: Cell::new(0),
                elapsed: Cell::new(-1),
                duration: Cell::new(-1),
            }
        }
    }

    impl Sound {
        fn display_name(&self) -> String {
            let custom = self.custom_name.borrow().clone();
            custom.unwrap_or_else(|| self.name.get().cloned().unwrap_or_default())
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Sound {
        const NAME: &'static str = "VinhetaSound";
        type Type = super::Sound;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Sound {}
}

glib::wrapper! {
    /// One audio file of the library, shown as a pad.
    pub struct Sound(ObjectSubclass<imp::Sound>);
}

fn seconds(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |time| i64::try_from(time.as_secs()).unwrap_or(i64::MAX))
}

impl Sound {
    pub fn new(path: &str, name: &str, modified: SystemTime, settings: &PadSettings) -> Self {
        let sound: Self = glib::Object::builder()
            .property("path", path)
            .property("name", name)
            .property("modified", seconds(modified))
            .build();
        sound.set_settings(settings);
        sound
    }

    pub fn file_name(&self) -> String {
        Path::new(&self.path())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Says whether the time is another one.
    pub fn set_modified_time(&self, modified: SystemTime) -> bool {
        let modified = seconds(modified);
        let changed = self.modified() != modified;
        if changed {
            self.set_modified(modified);
        }
        changed
    }

    pub fn modified_time(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.modified().max(0).unsigned_abs())
    }

    /// What the user set for this pad.
    pub fn settings(&self) -> PadSettings {
        let imp = self.imp();
        PadSettings {
            name: imp.custom_name.borrow().clone(),
            color: PadColor::from_name(&imp.color.borrow()),
            volume: imp.volume.get(),
            looping: imp.looping.get(),
            favorite: imp.favorite.get(),
            shortcut: imp.shortcut.borrow().chars().next(),
        }
    }

    /// Only the application calls this, which also stores the settings.
    pub fn set_settings(&self, settings: &PadSettings) {
        let imp = self.imp();
        let color = settings.color.map(PadColor::name).unwrap_or_default();
        if imp.custom_name.replace(settings.name.clone()) != settings.name {
            self.notify_display_name();
        }
        if imp.color.replace(color.to_owned()) != color {
            self.notify_color();
        }
        if imp.volume.replace(settings.volume) != settings.volume {
            self.notify_volume();
        }
        if imp.looping.replace(settings.looping) != settings.looping {
            self.notify_looping();
        }
        if imp.favorite.replace(settings.favorite) != settings.favorite {
            self.notify_favorite();
        }
        let shortcut = settings.shortcut.map(String::from).unwrap_or_default();
        if imp.shortcut.replace(shortcut.clone()) != shortcut {
            self.notify_shortcut();
        }
    }

    /// How far along the playback is, or `None` when it is not known.
    pub fn set_position(&self, position: Option<(i64, Option<i64>)>) {
        let imp = self.imp();
        let (elapsed, duration) = position.unwrap_or((-1, None));
        let duration = duration.unwrap_or(-1);
        if imp.duration.replace(duration) != duration {
            self.notify_duration();
        }
        if imp.elapsed.replace(elapsed) != elapsed {
            self.notify_elapsed();
        }
    }
}
