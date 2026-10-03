/* window.rs
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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use super::device_selector;
use super::folder_page::FolderPage;
use crate::application::{DeviceKind, VinhetaApplication};
use crate::sound::Sound;
use crate::APP_ID;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/window.ui")]
    pub struct VinhetaWindow {
        #[template_child]
        pub toasts: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub content: TemplateChild<gtk::Stack>,
        #[template_child]
        pub tabs: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub stop_all: TemplateChild<gtk::Button>,
        #[template_child]
        pub send_to_call: TemplateChild<gtk::Switch>,
        #[template_child]
        pub monitor_volume: TemplateChild<gtk::Adjustment>,
        #[template_child]
        pub monitor_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub call_volume: TemplateChild<gtk::Adjustment>,
        #[template_child]
        pub call_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub call_volume_row: TemplateChild<gtk::Box>,
        #[template_child]
        pub microphone: TemplateChild<gtk::DropDown>,
        pub settings: OnceCell<gio::Settings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VinhetaWindow {
        const NAME: &'static str = "VinhetaWindow";
        type Type = super::VinhetaWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for VinhetaWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let settings = gio::Settings::new(APP_ID);
            settings
                .bind("send-sounds-to-call", &*self.send_to_call, "active")
                .build();
            // The call volume has no effect while sounds are not sent.
            settings
                .bind("send-sounds-to-call", &*self.call_volume_row, "sensitive")
                .get_only()
                .build();
            for (key, volume, percent) in [
                ("monitor-volume", &self.monitor_volume, &self.monitor_percent),
                ("call-volume", &self.call_volume, &self.call_percent),
            ] {
                let percent = percent.get();
                volume.connect_value_changed(move |volume| {
                    let value = (volume.value() * 100.0).round();
                    // Translators: {} is a volume, from 0 to 100.
                    percent.set_label(&gettext("{}%").replace("{}", &value.to_string()));
                });
                settings.bind(key, &**volume, "value").build();
                volume.emit_by_name::<()>("value-changed", &[]);
            }
            if let Some(app) = gio::Application::default().and_downcast::<VinhetaApplication>() {
                device_selector::bind(&*self.microphone, &app, DeviceKind::Microphone);
            }
            let directories = settings.strv("directories");
            self.settings.set(settings).unwrap();

            obj.setup_gactions();
            for path in &directories {
                if self.tabs.child_by_name(path).is_none() {
                    obj.add_page(&FolderPage::new(path));
                }
            }
            obj.update_content();
        }
    }

    impl WidgetImpl for VinhetaWindow {}
    impl WindowImpl for VinhetaWindow {}
    impl ApplicationWindowImpl for VinhetaWindow {}
    impl AdwApplicationWindowImpl for VinhetaWindow {}
}

glib::wrapper! {
    pub struct VinhetaWindow(ObjectSubclass<imp::VinhetaWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl VinhetaWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    pub fn find_sound(&self, path: &str) -> Option<Sound> {
        self.pages()
            .iter()
            .flat_map(FolderPage::sounds)
            .find(|sound| sound.path() == path)
    }

    /// Shows or hides the banner that says audio does not work, and disables
    /// the pads along with it.
    pub fn set_audio_error(&self, message: Option<&str>) {
        let imp = self.imp();
        if let Some(message) = message {
            // Translators: {} is the reason reported by the audio system.
            let title = gettext("Audio is unavailable: {}").replace("{}", message);
            imp.banner.set_title(&title);
        }
        imp.banner.set_revealed(message.is_some());
        imp.tabs.set_sensitive(message.is_none());
    }

    pub fn set_any_playing(&self, playing: bool) {
        if playing {
            self.imp().stop_all.add_css_class("destructive-action");
        } else {
            self.imp().stop_all.remove_css_class("destructive-action");
        }
    }

    pub fn toast(&self, message: &str) {
        self.imp().toasts.add_toast(adw::Toast::new(message));
    }

    fn setup_gactions(&self) {
        let add_folder = gio::ActionEntry::builder("add-folder")
            .activate(|window: &Self, _, _| {
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    window,
                    async move { window.choose_folder().await }
                ));
            })
            .build();
        let remove_folder = gio::ActionEntry::builder("remove-folder")
            .activate(|window: &Self, _, _| window.remove_folder())
            .build();
        self.add_action_entries([add_folder, remove_folder]);
    }

    async fn choose_folder(&self) {
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Add Folder"))
            .modal(true)
            .build();
        let Ok(folder) = dialog.select_folder_future(Some(self)).await else {
            return;
        };
        // The list of folders is stored as strings.
        match folder.path().as_deref().and_then(|path| path.to_str()) {
            Some(path) => self.add_folder(path),
            None => self.toast(&gettext("This folder cannot be added")),
        }
    }

    fn add_folder(&self, path: &str) {
        let tabs = &self.imp().tabs;
        if tabs.child_by_name(path).is_none() {
            let mut directories = self.directories();
            directories.push(path.to_owned());
            self.set_directories(directories);
            self.add_page(&FolderPage::new(path));
            self.update_content();
        }
        tabs.set_visible_child_name(path);
    }

    /// Removes the tab being shown. Only the library entry goes away, never
    /// the files.
    fn remove_folder(&self) {
        let tabs = &self.imp().tabs;
        let Some(page) = tabs.visible_child().and_downcast::<FolderPage>() else {
            return;
        };
        let mut directories = self.directories();
        let Some(position) = directories.iter().position(|path| path == page.path()) else {
            return;
        };

        for sound in page.sounds().filter(Sound::playing) {
            let path = sound.path().to_variant();
            let _ = WidgetExt::activate_action(self, "app.toggle-sound", Some(&path));
        }
        directories.remove(position);
        self.set_directories(directories);

        let pages = self.pages();
        let neighbor = pages
            .get(position + 1)
            .or_else(|| pages.get(position.checked_sub(1)?));
        tabs.remove(&page);
        if let Some(neighbor) = neighbor {
            tabs.set_visible_child(neighbor);
        }
        self.update_content();

        // Translators: {} is the name of a folder.
        let toast = adw::Toast::new(&gettext("Removed “{}”").replace("{}", &page.title()));
        toast.set_button_label(Some(&gettext("Undo")));
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.restore_folder(&page, position)
        ));
        self.imp().toasts.add_toast(toast);
    }

    fn restore_folder(&self, page: &FolderPage, position: usize) {
        let tabs = &self.imp().tabs;
        if tabs.child_by_name(page.path()).is_none() {
            let mut directories = self.directories();
            let position = position.min(directories.len());
            directories.insert(position, page.path().to_owned());
            self.set_directories(directories);

            // The stack can only append, so the tabs after it are added again.
            let after = self.pages().split_off(position);
            for other in &after {
                tabs.remove(other);
            }
            self.add_page(page);
            for other in &after {
                self.add_page(other);
            }
            self.update_content();
        }
        tabs.set_visible_child(page);
    }

    fn add_page(&self, page: &FolderPage) {
        self.imp()
            .tabs
            .add_titled(page, Some(page.path()), &page.title());
    }

    /// The folder pages, in tab order.
    fn pages(&self) -> Vec<FolderPage> {
        let pages = self.imp().tabs.pages();
        pages
            .iter::<adw::ViewStackPage>()
            .flatten()
            .filter_map(|page| page.child().downcast().ok())
            .collect()
    }

    fn update_content(&self) {
        let empty = self.pages().is_empty();
        let name = if empty { "empty" } else { "library" };
        self.imp().content.set_visible_child_name(name);
        if let Some(action) = self.lookup_action("remove-folder") {
            if let Some(action) = action.downcast_ref::<gio::SimpleAction>() {
                action.set_enabled(!empty);
            }
        }
    }

    fn directories(&self) -> Vec<String> {
        let settings = self.imp().settings.get().unwrap();
        settings
            .strv("directories")
            .iter()
            .map(|path| path.to_string())
            .collect()
    }

    fn set_directories(&self, directories: Vec<String>) {
        let settings = self.imp().settings.get().unwrap();
        if let Err(error) = settings.set_strv("directories", directories) {
            glib::g_warning!("vinheta", "could not save the folders: {error}");
        }
    }
}
