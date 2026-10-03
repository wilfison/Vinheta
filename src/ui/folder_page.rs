/* folder_page.rs
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

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib};
use vinheta::library;

use super::sound_pad::SoundPad;
use crate::sound::Sound;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/folder-page.ui")]
    pub struct FolderPage {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub grid: TemplateChild<gtk::GridView>,
        #[template_child]
        pub missing: TemplateChild<adw::StatusPage>,
        pub path: OnceCell<String>,
        pub sounds: OnceCell<gio::ListStore>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FolderPage {
        const NAME: &'static str = "VinhetaFolderPage";
        type Type = super::FolderPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FolderPage {
        fn constructed(&self) {
            self.parent_constructed();

            let sounds = gio::ListStore::new::<Sound>();
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(|_, item| {
                if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                    item.set_child(Some(&SoundPad::default()));
                }
            });
            factory.connect_bind(|_, item| {
                if let Some((pad, sound)) = pad_and_sound(item) {
                    pad.set_sound(sound.as_ref());
                }
            });
            factory.connect_unbind(|_, item| {
                if let Some((pad, _)) = pad_and_sound(item) {
                    pad.set_sound(None);
                }
            });

            self.grid.set_factory(Some(&factory));
            self.grid
                .set_model(Some(&gtk::NoSelection::new(Some(sounds.clone()))));
            // Pads go through the application action, the same entry point
            // that anything outside the window uses.
            self.grid.connect_activate(|grid, position| {
                let sound = grid.model().and_then(|model| model.item(position));
                if let Some(sound) = sound.and_downcast::<Sound>() {
                    let _ = grid.activate_action("app.toggle-sound", Some(&sound.path().to_variant()));
                }
            });
            self.sounds.set(sounds).unwrap();
        }
    }

    fn pad_and_sound(item: &glib::Object) -> Option<(SoundPad, Option<Sound>)> {
        let item = item.downcast_ref::<gtk::ListItem>()?;
        let pad = item.child().and_downcast()?;
        Some((pad, item.item().and_downcast()))
    }

    impl WidgetImpl for FolderPage {}
    impl BinImpl for FolderPage {}
}

glib::wrapper! {
    /// The content of one tab: the pads of one folder.
    pub struct FolderPage(ObjectSubclass<imp::FolderPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FolderPage {
    pub fn new(path: &str) -> Self {
        let page: Self = glib::Object::new();
        page.imp().path.set(path.to_owned()).unwrap();
        page.load();
        page
    }

    pub fn path(&self) -> &str {
        self.imp().path.get().unwrap()
    }

    pub fn title(&self) -> String {
        Path::new(self.path())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path().to_owned())
    }

    pub fn sounds(&self) -> impl Iterator<Item = Sound> {
        let sounds = self.imp().sounds.get().unwrap();
        sounds.iter::<Sound>().flatten().collect::<Vec<_>>().into_iter()
    }

    fn load(&self) {
        let folder = PathBuf::from(self.path());
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = page)]
            self,
            async move {
                let scan = gio::spawn_blocking(move || library::scan(&folder)).await;
                let imp = page.imp();
                let Ok(Ok(files)) = scan else {
                    imp.missing.set_description(Some(page.path()));
                    imp.stack.set_visible_child_name("missing");
                    return;
                };
                // The action that plays a sound takes its path as a string.
                let sounds: Vec<_> = files
                    .iter()
                    .filter_map(|file| Some(Sound::new(file.path.to_str()?, &file.name)))
                    .collect();
                imp.sounds.get().unwrap().extend_from_slice(&sounds);
                let child = if sounds.is_empty() { "empty" } else { "grid" };
                imp.stack.set_visible_child_name(child);
            }
        ));
    }
}
