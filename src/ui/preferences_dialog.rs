/* preferences_dialog.rs
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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use super::device_selector;
use crate::application::{DeviceKind, VinhetaApplication};
use crate::APP_ID;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/preferences-dialog.ui")]
    pub struct PreferencesDialog {
        #[template_child]
        pub microphone: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub monitor_output: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub send_to_call: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub include_voice: TemplateChild<adw::SwitchRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesDialog {
        const NAME: &'static str = "VinhetaPreferencesDialog";
        type Type = super::PreferencesDialog;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let settings = gio::Settings::new(APP_ID);
            settings
                .bind("send-sounds-to-call", &*self.send_to_call, "active")
                .build();
            settings
                .bind("include-my-voice", &*self.include_voice, "active")
                .build();
        }
    }

    impl WidgetImpl for PreferencesDialog {}
    impl AdwDialogImpl for PreferencesDialog {}
    impl PreferencesDialogImpl for PreferencesDialog {}
}

glib::wrapper! {
    pub struct PreferencesDialog(ObjectSubclass<imp::PreferencesDialog>)
        @extends gtk::Widget, adw::Dialog, adw::PreferencesDialog;
}

impl PreferencesDialog {
    pub fn new(app: &VinhetaApplication) -> Self {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        device_selector::bind(&*imp.microphone, app, DeviceKind::Microphone);
        device_selector::bind(&*imp.monitor_output, app, DeviceKind::Output);
        dialog
    }
}
