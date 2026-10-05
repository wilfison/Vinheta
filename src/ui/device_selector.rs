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

//! Keeps a `GtkDropDown` or an `AdwComboRow` in sync with a list of the
//! application (the outputs, or the apps that are recording) and with the
//! settings key that stores the chosen entry.

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
    // The name of each entry, in the order shown.
    let names: Rc<RefCell<Vec<String>>> = Rc::default();
    // What is shown, to leave an open list alone when nothing changed.
    let shown: Rc<RefCell<(Vec<String>, usize)>> = Rc::default();
    // Set while the list is rebuilt, which moves the selection by itself.
    let refreshing = Rc::new(Cell::new(false));

    let refresh = {
        let names = names.clone();
        let refreshing = refreshing.clone();
        let selector = selector.downgrade();
        let app = app.downgrade();
        move || {
            let (Some(selector), Some(app)) = (selector.upgrade(), app.upgrade()) else {
                return;
            };
            let (entries, selected) = app.device_entries(kind);
            let labels: Vec<String> = entries
                .iter()
                .map(
                    |entry| match (kind, entry.name.is_empty(), entry.available) {
                        (DeviceKind::Output, true, _) => gettext("System Default"),
                        // Translators: the sounds are sent to every app that is using the microphone.
                        (DeviceKind::CallApp, true, _) => gettext("All Apps"),
                        (_, false, true) => entry.description.clone(),
                        (DeviceKind::Output, false, false) => {
                            // Translators: {} is the name of an audio device that is not connected.
                            gettext("{} (unavailable)").replace("{}", &entry.description)
                        }
                        (DeviceKind::CallApp, false, false) => {
                            // Translators: {} is the name of an app that is not using the microphone.
                            gettext("{} (not recording)").replace("{}", &entry.description)
                        }
                    },
                )
                .collect();
            selector.set_sensitive(app.audio_available());
            names.replace(entries.into_iter().map(|entry| entry.name).collect());
            if *shown.borrow() == (labels.clone(), selected) {
                return;
            }

            refreshing.set(true);
            let model: Vec<&str> = labels.iter().map(String::as_str).collect();
            selector.set_property("model", gtk::StringList::new(&model));
            selector.set_property("selected", selected as u32);
            refreshing.set(false);
            shown.replace((labels, selected));
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
            if key == kind.key() {
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
