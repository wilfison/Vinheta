/* sound_pad.rs
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

use std::cell::RefCell;
use std::path::Path;

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;

use crate::sound::Sound;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/sound-pad.ui")]
    pub struct SoundPad {
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
        pub sound: RefCell<Option<(Sound, glib::SignalHandlerId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SoundPad {
        const NAME: &'static str = "VinhetaSoundPad";
        type Type = super::SoundPad;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SoundPad {}
    impl WidgetImpl for SoundPad {}
    impl BinImpl for SoundPad {}
}

glib::wrapper! {
    pub struct SoundPad(ObjectSubclass<imp::SoundPad>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for SoundPad {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl SoundPad {
    /// Shows the given sound and follows its playing state, or lets go of the
    /// current one.
    pub fn set_sound(&self, sound: Option<&Sound>) {
        if let Some((old, handler)) = self.imp().sound.take() {
            old.disconnect(handler);
        }
        let Some(sound) = sound else { return };

        self.imp().label.set_label(&sound.name());
        let file_name = Path::new(&sound.path())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        self.set_tooltip_text(file_name.as_deref());

        let handler = sound.connect_playing_notify(glib::clone!(
            #[weak(rename_to = pad)]
            self,
            move |sound| pad.show_playing(sound.playing())
        ));
        self.show_playing(sound.playing());
        self.imp().sound.replace(Some((sound.clone(), handler)));
    }

    fn show_playing(&self, playing: bool) {
        if playing {
            self.add_css_class("playing");
            self.update_property(&[gtk::accessible::Property::Description(&gettext("Playing"))]);
        } else {
            self.remove_css_class("playing");
            self.reset_property(gtk::AccessibleProperty::Description);
        }
    }
}
