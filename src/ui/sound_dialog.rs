/* sound_dialog.rs
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

use std::cell::{Cell, RefCell};
use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, gio, glib};
use vinheta::pads::{self, PadColor, PadSettings};

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
        pub background_frame: TemplateChild<gtk::Overlay>,
        #[template_child]
        pub background_picture: TemplateChild<gtk::Picture>,
        #[template_child]
        pub background_choose: TemplateChild<gtk::Button>,
        #[template_child]
        pub background_remove: TemplateChild<gtk::Button>,
        #[template_child]
        pub background_adjust: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub volume: TemplateChild<gtk::Adjustment>,
        #[template_child]
        pub volume_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub looping: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub shortcut_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub shortcut_key: TemplateChild<gtk::Label>,
        #[template_child]
        pub shortcut_set: TemplateChild<gtk::Button>,
        #[template_child]
        pub shortcut_remove: TemplateChild<gtk::Button>,
        /// Set while the next key pressed becomes the key of the pad.
        pub listening: Cell<bool>,
        /// The key taken from another pad while the dialog is open, and the
        /// name of that pad.
        pub taken: RefCell<Option<(char, String)>>,
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

            self.background_choose.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        obj,
                        async move { obj.choose_background().await }
                    ));
                }
            ));
            self.background_remove.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| obj.background_action("set-background", "")
            ));
            self.background_adjust.connect_activated(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    if let Some(sound) = obj.sound() {
                        let path = sound.path().to_variant();
                        let _ = obj.activate_action("win.adjust-background", Some(&path));
                    }
                }
            ));

            self.shortcut_set.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| obj.set_listening(!obj.imp().listening.get())
            ));
            self.shortcut_remove.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    obj.set_listening(false);
                    obj.change(|settings| settings.shortcut = None);
                }
            ));
            // Before the dialog itself, which closes on Escape.
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(glib::clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, keyval, _, state| obj.key_pressed(keyval, state)
            ));
            obj.add_controller(keys);
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

    /// The key of this pad belonged to `from` until now.
    pub fn shortcut_taken(&self, key: char, from: &Sound) {
        let taken = (key, from.display_name());
        self.imp().taken.replace(Some(taken));
    }

    fn set_listening(&self, listening: bool) {
        self.imp().listening.set(listening);
        if let Some(sound) = self.sound() {
            self.show_shortcut(sound.settings().shortcut);
        }
    }

    /// While listening, every key is for the row.
    fn key_pressed(&self, keyval: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        if !self.imp().listening.get() {
            return glib::Propagation::Proceed;
        }
        let modifiers = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK;
        let key = keyval.to_unicode().and_then(pads::shortcut_key);
        match keyval {
            gdk::Key::Escape => self.set_listening(false),
            gdk::Key::BackSpace | gdk::Key::Delete | gdk::Key::KP_Delete => {
                self.set_listening(false);
                self.change(|settings| settings.shortcut = None);
            }
            _ => {
                if let Some(key) = key.filter(|_| !state.intersects(modifiers)) {
                    self.set_listening(false);
                    self.change(|settings| settings.shortcut = Some(key));
                }
            }
        }
        glib::Propagation::Stop
    }

    fn show_shortcut(&self, key: Option<char>) {
        let imp = self.imp();
        let listening = imp.listening.get();
        match key {
            Some(key) => {
                imp.shortcut_key.set_label(&pads::shortcut_label(key));
                imp.shortcut_key.remove_css_class("dim-label");
                imp.shortcut_key.add_css_class("keycap");
            }
            None => {
                imp.shortcut_key.set_label(&gettext("None"));
                imp.shortcut_key.remove_css_class("keycap");
                imp.shortcut_key.add_css_class("dim-label");
            }
        }
        imp.shortcut_remove.set_visible(key.is_some() && !listening);
        imp.shortcut_set.set_label(&match (listening, key) {
            (true, _) => gettext("Cancel"),
            (false, Some(_)) => gettext("Change…"),
            (false, None) => gettext("Set…"),
        });
        let taken = imp.taken.borrow();
        let taken = taken.as_ref().filter(|(taken, _)| Some(*taken) == key);
        let subtitle = match (listening, taken) {
            (true, _) => gettext("Press a letter or a digit"),
            // Translators: {} is the name of the pad that had the key before.
            (false, Some((_, name))) => gettext("Taken from “{}”").replace("{}", name),
            (false, None) => gettext("Plays this pad while the window is focused"),
        };
        imp.shortcut_row
            .set_subtitle(&glib::markup_escape_text(&subtitle));
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
        let background = settings.background.as_deref().unwrap_or_default();
        let texture = imp
            .app
            .upgrade()
            .and_then(|app| app.background_texture(background));
        imp.background_picture.set_paintable(texture.as_ref());
        imp.background_frame.set_visible(texture.is_some());
        imp.background_remove
            .set_visible(settings.background.is_some());
        imp.background_adjust.set_visible(texture.is_some());
        imp.background_choose.set_label(&match settings.background {
            Some(_) => gettext("Change…"),
            None => gettext("Choose…"),
        });
        self.show_shortcut(settings.shortcut);
        imp.filling.set(false);
    }

    /// The dialog stays open: the row shows the picture once it is stored.
    async fn choose_background(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Images")));
        filter.add_pixbuf_formats();
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Choose Image"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let root = self.root().and_downcast::<gtk::Window>();
        let Ok(file) = chooser.open_future(root.as_ref()).await else {
            return;
        };
        if let Some(path) = file.path().as_deref().and_then(|path| path.to_str()) {
            self.background_action("add-background", path);
        }
    }

    /// Activates an action of the application with the path of the sound and
    /// an image (empty to remove the background).
    fn background_action(&self, action: &str, image: &str) {
        let imp = self.imp();
        let (Some(app), Some(sound)) = (imp.app.upgrade(), self.sound()) else {
            return;
        };
        let parameter = (sound.path(), image.to_owned()).to_variant();
        app.activate_action(action, Some(&parameter));
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
