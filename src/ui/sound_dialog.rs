/* sound_dialog.rs
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

use std::cell::{Cell, RefCell};
use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use vinheta::pads::{PadColor, PadSettings};

use crate::application::VinhetaApplication;
use crate::sound::Sound;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/sound-dialog.ui")]
    pub struct SoundDialog {
        #[template_child]
        pub title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub name: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub swatches: TemplateChild<gtk::Box>,
        #[template_child]
        pub volume: TemplateChild<gtk::Adjustment>,
        #[template_child]
        pub volume_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub looping: TemplateChild<adw::SwitchRow>,
        /// "No color" first, then the palette.
        pub colors: RefCell<Vec<(Option<PadColor>, gtk::ToggleButton)>>,
        pub sound: RefCell<Option<(Sound, glib::SignalHandlerId)>>,
        pub app: glib::WeakRef<VinhetaApplication>,
        /// Set while the controls are being filled from the sound.
        pub filling: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SoundDialog {
        const NAME: &'static str = "VinhetaSoundDialog";
        type Type = super::SoundDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SoundDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let mut colors: Vec<(Option<PadColor>, gtk::ToggleButton)> = Vec::new();
            for color in std::iter::once(None).chain(PadColor::ALL.map(Some)) {
                let label = color_label(color);
                let swatch = gtk::ToggleButton::builder()
                    .tooltip_text(&label)
                    .child(&gtk::Image::from_icon_name("object-select-symbolic"))
                    .css_classes(["swatch", "circular", color.map_or("none", PadColor::name)])
                    .build();
                swatch.update_property(&[gtk::accessible::Property::Label(&label)]);
                if let Some((_, first)) = colors.first() {
                    swatch.set_group(Some(first));
                }
                swatch.connect_toggled(glib::clone!(
                    #[weak]
                    obj,
                    move |swatch| {
                        if swatch.is_active() {
                            obj.change(|settings| settings.color = color);
                        }
                    }
                ));
                self.swatches.append(&swatch);
                colors.push((color, swatch));
            }
            self.colors.replace(colors);

            self.name.connect_changed(glib::clone!(
                #[weak]
                obj,
                move |name| {
                    let text = name.text().to_string();
                    obj.change(|settings| settings.name = Some(text));
                }
            ));
            self.volume.connect_value_changed(glib::clone!(
                #[weak]
                obj,
                move |volume| {
                    let value = (volume.value() * 100.0).round();
                    // Translators: {} is a volume, from 0 to 100.
                    let percent = gettext("{}%").replace("{}", &value.to_string());
                    obj.imp().volume_percent.set_label(&percent);
                    obj.change(|settings| settings.volume = volume.value());
                }
            ));
            self.looping.connect_active_notify(glib::clone!(
                #[weak]
                obj,
                move |looping| obj.change(|settings| settings.looping = looping.is_active())
            ));
        }

        fn dispose(&self) {
            if let Some((sound, handler)) = self.sound.take() {
                sound.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for SoundDialog {}
    impl AdwDialogImpl for SoundDialog {}
}

fn color_label(color: Option<PadColor>) -> String {
    match color {
        None => gettext("No Color"),
        Some(PadColor::Blue) => gettext("Blue"),
        Some(PadColor::Green) => gettext("Green"),
        Some(PadColor::Yellow) => gettext("Yellow"),
        Some(PadColor::Orange) => gettext("Orange"),
        Some(PadColor::Red) => gettext("Red"),
        Some(PadColor::Purple) => gettext("Purple"),
        Some(PadColor::Brown) => gettext("Brown"),
    }
}

glib::wrapper! {
    /// Edits the name, the color, the volume, and the loop of one pad. Every
    /// control acts at once, through the application.
    pub struct SoundDialog(ObjectSubclass<imp::SoundDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl SoundDialog {
    pub fn new(app: &VinhetaApplication, sound: &Sound) -> Self {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.app.set(Some(app));

        let file_name = Path::new(&sound.path())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        imp.title.set_subtitle(&file_name);

        // The loop can be switched from the context menu, and "Reset" changes
        // everything, while the dialog is open.
        let handler = sound.connect_notify_local(
            None,
            glib::clone!(
                #[weak]
                dialog,
                move |sound, property| {
                    if !matches!(property.name(), "elapsed" | "duration" | "playing") {
                        dialog.fill(&sound.settings());
                    }
                }
            ),
        );
        dialog.fill(&sound.settings());
        imp.sound.replace(Some((sound.clone(), handler)));
        dialog
    }

    pub fn sound(&self) -> Option<Sound> {
        let sound = self.imp().sound.borrow();
        sound.as_ref().map(|(sound, _)| sound.clone())
    }

    fn fill(&self, settings: &PadSettings) {
        let imp = self.imp();
        imp.filling.set(true);
        // The field is left alone while it says the same as the stored name,
        // which is trimmed: the user may be typing a space.
        let name = settings.name.as_deref().unwrap_or_default();
        if imp.name.text().trim() != name {
            imp.name.set_text(name);
        }
        for (color, swatch) in imp.colors.borrow().iter() {
            if *color == settings.color {
                swatch.set_active(true);
            }
        }
        imp.volume.set_value(settings.volume);
        imp.volume.emit_by_name::<()>("value-changed", &[]);
        imp.looping.set_active(settings.looping);
        imp.filling.set(false);
    }

    fn change(&self, change: impl FnOnce(&mut PadSettings)) {
        let imp = self.imp();
        if imp.filling.get() {
            return;
        }
        let sound = imp.sound.borrow().as_ref().map(|(sound, _)| sound.clone());
        let (Some(app), Some(sound)) = (imp.app.upgrade(), sound) else {
            return;
        };
        let mut settings = sound.settings();
        change(&mut settings);
        app.update_sound(&sound, settings);
    }
}
