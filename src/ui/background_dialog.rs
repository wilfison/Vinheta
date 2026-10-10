/* background_dialog.rs
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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib};

use crate::application::VinhetaApplication;
use crate::sound::Sound;
use crate::ui::background_editor::BackgroundEditor;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/background-dialog.ui")]
    pub struct BackgroundDialog {
        #[template_child]
        pub title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub cancel: TemplateChild<gtk::Button>,
        #[template_child]
        pub apply: TemplateChild<gtk::Button>,
        #[template_child]
        pub editor: TemplateChild<BackgroundEditor>,
        #[template_child]
        pub zoom: TemplateChild<gtk::Adjustment>,
        pub sound: RefCell<Option<(Sound, glib::SignalHandlerId)>>,
        pub app: glib::WeakRef<VinhetaApplication>,
        /// Set while the slider is moved to follow the editor.
        pub following: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BackgroundDialog {
        const NAME: &'static str = "VinhetaBackgroundDialog";
        type Type = super::BackgroundDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            BackgroundEditor::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for BackgroundDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.cancel.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    obj.close();
                }
            ));
            self.apply.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| obj.apply()
            ));
            // The default widget is not activated while the editor or the
            // slider has the focus; a button that has it takes Return first.
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(glib::clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, keyval, _, _| match keyval {
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                        obj.apply();
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            ));
            obj.add_controller(keys);
            self.editor.connect_changed(glib::clone!(
                #[weak]
                obj,
                move |editor| {
                    let imp = obj.imp();
                    imp.following.set(true);
                    imp.zoom.set_value(editor.zoom());
                    imp.following.set(false);
                }
            ));
            self.zoom.connect_value_changed(glib::clone!(
                #[weak]
                obj,
                move |zoom| {
                    let imp = obj.imp();
                    if !imp.following.get() {
                        imp.editor.set_zoom(zoom.value());
                    }
                }
            ));
        }

        fn dispose(&self) {
            if let Some((sound, handler)) = self.sound.take() {
                sound.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for BackgroundDialog {}
    impl AdwDialogImpl for BackgroundDialog {}
}

glib::wrapper! {
    /// Chooses the area of its background a pad shows. Nothing changes until
    /// "Apply".
    pub struct BackgroundDialog(ObjectSubclass<imp::BackgroundDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl BackgroundDialog {
    pub fn new(app: &VinhetaApplication, sound: &Sound, texture: &gdk::Texture) -> Self {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.app.set(Some(app));
        imp.title.set_subtitle(&sound.display_name());
        imp.editor.set_picture(texture, sound.crop_area());

        // The picture being adjusted is gone.
        let handler = sound.connect_background_notify(glib::clone!(
            #[weak]
            dialog,
            move |_| {
                dialog.close();
            }
        ));
        imp.sound.replace(Some((sound.clone(), handler)));
        dialog
    }

    fn apply(&self) {
        let imp = self.imp();
        let sound = imp.sound.borrow().as_ref().map(|(sound, _)| sound.clone());
        if let (Some(app), Some(sound)) = (imp.app.upgrade(), sound) {
            let mut settings = sound.settings();
            settings.crop = Some(imp.editor.crop());
            app.update_sound(&sound, settings);
        }
        self.close();
    }
}
