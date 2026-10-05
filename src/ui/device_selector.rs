/* device_selector.rs
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

//! Keeps a `GtkDropDown` or an `AdwComboRow` in sync with a device list of
//! the application and with the settings key that stores the chosen device.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::application::{DeviceKind, VinhetaApplication};
use crate::APP_ID;

/// `selector` must have the `model` and `selected` properties.
pub fn bind(selector: &impl IsA<gtk::Widget>, app: &VinhetaApplication, kind: DeviceKind) {
    let selector = selector.upcast_ref::<gtk::Widget>();
    let settings = gio::Settings::new(APP_ID);
    // The node name of each entry, in the order shown.
    let names: Rc<RefCell<Vec<String>>> = Rc::default();
    // Set while the list is rebuilt, which moves the selection by itself.
    let refreshing = Rc::new(Cell::new(false));

    let refresh = {
        let names = names.clone();
        let refreshing = refreshing.clone();
        let settings = settings.clone();
        let selector = selector.downgrade();
        let app = app.downgrade();
        move || {
            let (Some(selector), Some(app)) = (selector.upgrade(), app.upgrade()) else {
                return;
            };
            let (entries, selected) = app.device_entries(kind);
            let labels: Vec<String> = entries
                .iter()
                .map(|entry| match (entry.name.is_empty(), entry.available) {
                    (true, _) => gettext("System Default"),
                    (false, true) => entry.description.clone(),
                    // Translators: {} is the name of an audio device that is not connected.
                    (false, false) => gettext("{} (unavailable)").replace("{}", &entry.description),
                })
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();

            refreshing.set(true);
            names.replace(entries.into_iter().map(|entry| entry.name).collect());
            selector.set_property("model", gtk::StringList::new(&labels));
            selector.set_property("selected", selected as u32);
            refreshing.set(false);

            // The microphone has no effect while the voice is off.
            let used = kind != DeviceKind::Microphone || settings.boolean("include-my-voice");
            selector.set_sensitive(app.audio_available() && used);
        }
    };
    refresh();

    selector.connect_notify_local(Some("selected"), {
        let settings = settings.clone();
        move |selector, _| {
            if refreshing.get() {
                return;
            }
            let selected = selector.property::<u32>("selected") as usize;
            let Some(name) = names.borrow().get(selected).cloned() else {
                return;
            };
            if settings.string(kind.key()) != name {
                let _ = settings.set_string(kind.key(), &name);
            }
        }
    });

    // The selector keeps the settings object alive through the handler above,
    // and these handlers go away with it.
    settings.connect_changed(None, {
        let refresh = refresh.clone();
        move |_, key| {
            // Not at once: the key may be changing from inside the activation
            // of an entry, and the list must outlive it.
            if key == kind.key() || key == "include-my-voice" {
                glib::idle_add_local_once(refresh.clone());
            }
        }
    });

    // Also emitted at once when the key changes, so it is deferred as well.
    let handler = app.connect_local("devices-changed", false, move |_| {
        glib::idle_add_local_once(refresh.clone());
        None
    });
    let app = app.downgrade();
    let handler = RefCell::new(Some(handler));
    selector.connect_destroy(move |_| {
        if let (Some(app), Some(handler)) = (app.upgrade(), handler.take()) {
            app.disconnect(handler);
        }
    });
}
