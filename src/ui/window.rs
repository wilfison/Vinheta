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

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gdk, gio, glib};
use vinheta::library::{self, ImportReport, SkipReason};
use vinheta::pads::{self, SortKey, SortOrder};

use super::device_selector;
use super::folder_page::FolderPage;
use super::sound_dialog::SoundDialog;
use super::sound_grid::SoundGrid;
use crate::application::{DeviceKind, VinhetaApplication};
use crate::sound::Sound;
use crate::APP_ID;

/// The name of the "Favorites" page. Folder pages are named by their path,
/// which is absolute, so it cannot collide.
const FAVORITES: &str = "favorites";

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
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub drop_hint: TemplateChild<gtk::Label>,
        #[template_child]
        pub content: TemplateChild<gtk::Stack>,
        #[template_child]
        pub switcher: TemplateChild<adw::InlineViewSwitcher>,
        #[template_child]
        pub views: TemplateChild<gtk::Stack>,
        #[template_child]
        pub tabs: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub results: TemplateChild<SoundGrid>,
        #[template_child]
        pub narrow: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub stop_all: TemplateChild<gtk::Button>,
        #[template_child]
        pub playing_counter: TemplateChild<gtk::Box>,
        #[template_child]
        pub playing_count: TemplateChild<gtk::Label>,
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
        pub sound_dialog: glib::WeakRef<SoundDialog>,
        /// The order of the pads in every view.
        pub sorter: OnceCell<gtk::CustomSorter>,
        pub sort_order: Rc<Cell<SortOrder>>,
        /// The stores of the folder pages, in tab order. Flattened, they are
        /// what the favorites and the search results filter.
        pub folders: OnceCell<gio::ListStore>,
        pub favorites: OnceCell<SoundGrid>,
        pub favorites_filter: OnceCell<gtk::CustomFilter>,
        pub favorites_model: OnceCell<gtk::FilterListModel>,
        pub results_filter: OnceCell<gtk::CustomFilter>,
        pub results_model: OnceCell<gtk::FilterListModel>,
        /// What the results match, empty while the tabs are shown.
        pub query: Rc<RefCell<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VinhetaWindow {
        const NAME: &'static str = "VinhetaWindow";
        type Type = super::VinhetaWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            SoundGrid::ensure_type();
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
                (
                    "monitor-volume",
                    &self.monitor_volume,
                    &self.monitor_percent,
                ),
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
            if let Some(app) = app() {
                device_selector::bind(&*self.microphone, &app, DeviceKind::Microphone);
            }
            let directories = settings.strv("directories");
            self.settings.set(settings).unwrap();

            obj.setup_models();
            obj.setup_search();
            obj.setup_drop();
            obj.setup_gactions();
            obj.setup_settings();

            // The style of the narrow layout: smaller pads and paddings.
            self.narrow.connect_apply(glib::clone!(
                #[weak]
                obj,
                move |_| obj.add_css_class("narrow")
            ));
            self.narrow.connect_unapply(glib::clone!(
                #[weak]
                obj,
                move |_| obj.remove_css_class("narrow")
            ));

            for path in &directories {
                if self.tabs.child_by_name(path).is_none() {
                    obj.append_page(&obj.new_page(path));
                }
            }
            self.tabs.connect_visible_child_notify(glib::clone!(
                #[weak]
                obj,
                move |_| obj.update_actions()
            ));
            obj.pages_changed();
            if let Some(first) = obj.pages().first() {
                self.tabs.set_visible_child(first);
            }
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

/// The window does not know its application while it is being built.
fn app() -> Option<VinhetaApplication> {
    gio::Application::default().and_downcast()
}

fn sort_key<'a>(name: &'a str, file_name: &'a str, sound: &Sound) -> SortKey<'a> {
    SortKey {
        display_name: name,
        file_name,
        modified: sound.modified_time(),
    }
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

    /// Shows or hides the banner that says audio does not work, and dims the
    /// pads along with it (the application ignores the triggers).
    pub fn set_audio_error(&self, message: Option<&str>) {
        let imp = self.imp();
        if let Some(message) = message {
            // Translators: {} is the reason reported by the audio system.
            let title = gettext("Audio is unavailable: {}").replace("{}", message);
            imp.banner.set_title(&title);
        }
        imp.banner.set_revealed(message.is_some());
        // The pads stay reachable: their settings can still be edited.
        if message.is_some() {
            imp.views.add_css_class("no-audio");
        } else {
            imp.views.remove_css_class("no-audio");
        }
    }

    /// Shows how many sounds are playing, in every tab, or nothing for none.
    pub fn set_playing_count(&self, count: usize) {
        let imp = self.imp();
        imp.playing_counter.set_visible(count > 0);
        let number = u32::try_from(count).unwrap_or(u32::MAX);
        // Translators: {} is how many sounds are playing.
        let text = ngettext("{} playing", "{} playing", number).replace("{}", &count.to_string());
        imp.playing_count.set_label(&text);
        if count > 0 {
            self.imp().stop_all.add_css_class("destructive-action");
        } else {
            self.imp().stop_all.remove_css_class("destructive-action");
        }
    }

    pub fn toast(&self, message: &str) {
        self.imp().toasts.add_toast(adw::Toast::new(message));
    }

    /// The sorted and filtered views do not watch their sounds: the
    /// application says when a name or a favorite changed.
    pub fn sound_changed(&self, name: bool, favorite: bool) {
        let imp = self.imp();
        if name {
            imp.sorter
                .get()
                .unwrap()
                .changed(gtk::SorterChange::Different);
            imp.results_filter
                .get()
                .unwrap()
                .changed(gtk::FilterChange::Different);
        }
        if favorite {
            imp.favorites_filter
                .get()
                .unwrap()
                .changed(gtk::FilterChange::Different);
        }
    }

    /// The file of the sound went away: the dialog that edits it closes.
    pub fn sound_gone(&self, sound: &Sound) {
        if let Some(dialog) = self.imp().sound_dialog.upgrade() {
            if dialog.sound().as_ref() == Some(sound) {
                dialog.force_close();
            }
        }
    }

    /// The models behind the "Favorites" tab and the search results: both
    /// filter the sounds of every folder, the same objects the tabs show.
    fn setup_models(&self) {
        let imp = self.imp();
        let settings = imp.settings.get().unwrap();
        imp.sort_order
            .set(SortOrder::from_name(&settings.string("sort-order")));
        let order = imp.sort_order.clone();
        let sorter = gtk::CustomSorter::new(move |a, b| {
            let (Some(a), Some(b)) = (a.downcast_ref::<Sound>(), b.downcast_ref::<Sound>()) else {
                return gtk::Ordering::Equal;
            };
            let names = (
                a.display_name(),
                a.file_name(),
                b.display_name(),
                b.file_name(),
            );
            let (a, b) = (
                sort_key(&names.0, &names.1, a),
                sort_key(&names.2, &names.3, b),
            );
            pads::compare(order.get(), a, b).into()
        });

        let folders = gio::ListStore::new::<gio::ListModel>();
        let all = gtk::FlattenListModel::new(Some(folders.clone()));

        let filter = gtk::CustomFilter::new(|sound| {
            sound.downcast_ref::<Sound>().is_some_and(Sound::favorite)
        });
        let favorites = gtk::FilterListModel::new(Some(all.clone()), Some(filter.clone()));
        let grid = SoundGrid::default();
        grid.set_model(&gtk::SortListModel::new(
            Some(favorites.clone()),
            Some(sorter.clone()),
        ));
        favorites.connect_items_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| window.update_favorites()
        ));
        imp.favorites.set(grid).unwrap();
        imp.favorites_filter.set(filter).unwrap();
        imp.favorites_model.set(favorites).unwrap();

        let query = imp.query.clone();
        let filter = gtk::CustomFilter::new(move |sound| {
            sound.downcast_ref::<Sound>().is_some_and(|sound| {
                pads::matches(&query.borrow(), &sound.display_name(), &sound.file_name())
            })
        });
        let results = gtk::FilterListModel::new(Some(all), Some(filter.clone()));
        imp.results.set_model(&gtk::SortListModel::new(
            Some(results.clone()),
            Some(sorter.clone()),
        ));
        imp.results.set_activate_action("win.activate-result");
        results.connect_items_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| window.update_view()
        ));
        imp.results_filter.set(filter).unwrap();
        imp.results_model.set(results).unwrap();

        imp.sorter.set(sorter).unwrap();
        imp.folders.set(folders).unwrap();
    }

    /// The search state is the bar being open and the text of its entry.
    fn setup_search(&self) {
        let imp = self.imp();
        imp.search_bar.connect_entry(&*imp.search_entry);
        imp.search_bar
            .connect_search_mode_enabled_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |bar| {
                    if !bar.is_search_mode() {
                        window.imp().search_entry.set_text("");
                    }
                    window.search_changed();
                }
            ));
        imp.search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.search_changed()
        ));
        // Enter triggers the first result.
        imp.search_entry.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.search_changed();
                let first = window.imp().results.first_sound();
                if let Some(sound) = first.filter(|_| window.searching()) {
                    window.activate_result(&sound.path());
                }
            }
        ));
    }

    /// Files and folders dropped anywhere on the window are added.
    fn setup_drop(&self) {
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let Ok(files) = value.get::<gdk::FileList>() else {
                    return false;
                };
                let paths = files.files().iter().filter_map(gio::File::path).collect();
                window.import_files(paths);
                true
            }
        ));
        drop.connect_current_drop_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |drop| window
                .imp()
                .drop_hint
                .set_visible(drop.current_drop().is_some())
        ));
        self.add_controller(drop);
    }

    fn setup_settings(&self) {
        let settings = self.imp().settings.get().unwrap();
        settings.connect_changed(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |settings, key| match key {
                    "sort-order" => {
                        let imp = window.imp();
                        imp.sort_order
                            .set(SortOrder::from_name(&settings.string(key)));
                        imp.sorter
                            .get()
                            .unwrap()
                            .changed(gtk::SorterChange::Different);
                    }
                    "folder-names" | "sounds-folder" => window.update_titles(),
                    _ => {}
                }
            ),
        );
    }

    fn setup_gactions(&self) {
        let imp = self.imp();
        let add_folder = gio::ActionEntry::builder("add-folder")
            .activate(|window: &Self, _, _| {
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    window,
                    async move { window.choose_folder().await }
                ));
            })
            .build();
        let add_sounds = gio::ActionEntry::builder("add-sounds")
            .activate(|window: &Self, _, _| {
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    window,
                    async move { window.choose_sounds().await }
                ));
            })
            .build();
        // The one entry point for loose files and dropped folders. The
        // parameter is a list of absolute paths.
        let import_files = gio::ActionEntry::builder("import-files")
            .parameter_type(Some(glib::VariantTy::STRING_ARRAY))
            .activate(|window: &Self, _, paths| {
                let paths = paths.and_then(|paths| paths.get::<Vec<String>>());
                let paths = paths.unwrap_or_default();
                window.import_files(paths.into_iter().map(PathBuf::from).collect());
            })
            .build();
        let remove_folder = gio::ActionEntry::builder("remove-folder")
            .activate(|window: &Self, _, _| window.remove_folder())
            .build();
        let rename_folder = gio::ActionEntry::builder("rename-folder")
            .activate(|window: &Self, _, _| {
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    window,
                    async move { window.rename_folder().await }
                ));
            })
            .build();
        let move_left = gio::ActionEntry::builder("move-folder-left")
            .activate(|window: &Self, _, _| window.move_folder(false))
            .build();
        let move_right = gio::ActionEntry::builder("move-folder-right")
            .activate(|window: &Self, _, _| window.move_folder(true))
            .build();
        // The parameter of the next three is the absolute path of a sound of
        // the library.
        let path_action = |name: &str, activate: fn(&Self, &str)| {
            gio::ActionEntry::builder(name)
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(move |window: &Self, _, path| {
                    if let Some(path) = path.and_then(|path| path.str()) {
                        activate(window, path);
                    }
                })
                .build()
        };
        // Opens the search with a text, or closes it with an empty one.
        let search = path_action("search", Self::search);
        self.add_action_entries([
            add_folder,
            add_sounds,
            import_files,
            remove_folder,
            rename_folder,
            move_left,
            move_right,
            search,
            path_action("edit-sound", Self::edit_sound),
            path_action("locate-sound", Self::locate_sound),
            path_action("activate-result", Self::activate_result),
        ]);
        self.add_action(&imp.settings.get().unwrap().create_action("sort-order"));
        // What the search button and Ctrl+F toggle.
        let search_mode =
            gio::PropertyAction::new("search-mode", &*imp.search_bar, "search-mode-enabled");
        self.add_action(&search_mode);
    }

    fn edit_sound(&self, path: &str) {
        let imp = self.imp();
        if imp.sound_dialog.upgrade().is_some() {
            return;
        }
        let app = app();
        let (Some(app), Some(sound)) = (app, self.find_sound(path)) else {
            return;
        };
        let dialog = SoundDialog::new(&app, &sound);
        imp.sound_dialog.set(Some(&dialog));
        dialog.present(Some(self));
    }

    fn search(&self, text: &str) {
        let imp = self.imp();
        imp.search_bar.set_search_mode(!text.is_empty());
        imp.search_entry.set_text(text);
        // The entry reports its changes after a delay.
        self.search_changed();
    }

    /// Whether the results are shown in place of the tabs.
    fn searching(&self) -> bool {
        !self.imp().query.borrow().is_empty()
    }

    fn search_changed(&self) {
        let imp = self.imp();
        let text = imp.search_entry.text();
        let query = if imp.search_bar.is_search_mode() {
            text.trim()
        } else {
            ""
        };
        if *imp.query.borrow() == query {
            return;
        }
        imp.query.replace(query.to_owned());
        imp.results_filter
            .get()
            .unwrap()
            .changed(gtk::FilterChange::Different);
        self.update_view();
        self.update_actions();
    }

    fn update_view(&self) {
        let imp = self.imp();
        let found = imp.results_model.get().unwrap().n_items() > 0;
        let view = match (self.searching(), found) {
            (false, _) => "tabs",
            (true, true) => "results",
            (true, false) => "no-results",
        };
        imp.views.set_visible_child_name(view);
        // The row stays for the playing counter.
        imp.switcher.set_visible(!self.searching());
    }

    /// Plays a result, closes the search, and shows where the sound is.
    fn activate_result(&self, path: &str) {
        let _ = WidgetExt::activate_action(self, "app.play-sound", Some(&path.to_variant()));
        self.imp().search_bar.set_search_mode(false);
        self.locate_sound(path);
    }

    /// Shows the tab of a sound, scrolls to its pad, and blinks it.
    fn locate_sound(&self, path: &str) {
        let pages = self.pages();
        let found = pages.iter().find_map(|page| {
            let sound = page.sounds().find(|sound| sound.path() == path)?;
            Some((page, sound))
        });
        if let Some((page, sound)) = found {
            self.imp().tabs.set_visible_child(page);
            page.locate(&sound);
        }
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

    async fn choose_sounds(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Audio Files")));
        for extension in library::EXTENSIONS {
            filter.add_suffix(extension);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Add Sounds"))
            .modal(true)
            .filters(&filters)
            .build();
        let Ok(files) = dialog.open_multiple_future(Some(self)).await else {
            return;
        };
        let files = files.iter::<gio::File>().flatten();
        self.import_files(files.filter_map(|file| file.path()).collect());
    }

    /// Folders become library folders; files are copied into the sounds
    /// folder, which is a library folder too.
    fn import_files(&self, paths: Vec<PathBuf>) {
        let (folders, files): (Vec<_>, Vec<_>) = paths.into_iter().partition(|path| path.is_dir());
        for folder in &folders {
            match folder.to_str() {
                Some(path) => self.add_folder(path),
                None => self.toast(&gettext("This folder cannot be added")),
            }
        }
        let app = app();
        let Some(app) = app.filter(|_| !files.is_empty()) else {
            return;
        };
        let folder = app.sounds_folder();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                let target = folder.clone();
                let report = gio::spawn_blocking(move || library::import(&files, &target)).await;
                if let Ok(report) = report {
                    window.files_imported(&report, &folder);
                }
            }
        ));
    }

    fn files_imported(&self, report: &ImportReport, folder: &Path) {
        let (copied, skipped) = (report.copied.len(), report.skipped.len());
        let mut failed = false;
        for (file, reason) in &report.skipped {
            if let SkipReason::Copy(message) = reason {
                glib::g_warning!("vinheta", "could not copy {}: {message}", file.display());
                failed = true;
            }
        }
        if let Some(path) = folder.to_str().filter(|_| copied > 0) {
            self.add_folder(path);
            // Not left to the monitor: the folder may have just been created.
            if let Some(page) = self.pages().iter().find(|page| page.path() == path) {
                page.rescan();
            }
        }

        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let added = ngettext("{} sound added", "{} sounds added", count(copied))
            .replace("{}", &copied.to_string());
        let left = ngettext("{} file skipped", "{} files skipped", count(skipped))
            .replace("{}", &skipped.to_string());
        let already = |(_, reason): &(PathBuf, SkipReason)| *reason == SkipReason::AlreadyThere;
        let message = if copied > 0 && skipped == 0 {
            added
        } else if copied > 0 {
            // Translators: the first {} is "2 sounds added", the second is
            // "1 file skipped".
            gettext("{}, {}")
                .replacen("{}", &added, 1)
                .replacen("{}", &left, 1)
        } else if report.skipped.iter().all(already) {
            // A file dropped on its own folder changes nothing.
            return;
        } else if failed {
            left
        } else {
            gettext("No supported audio files")
        };
        self.toast(&message);
    }

    fn new_page(&self, path: &str) -> FolderPage {
        FolderPage::new(path, self.imp().sorter.get().unwrap())
    }

    fn add_folder(&self, path: &str) {
        let tabs = &self.imp().tabs;
        if tabs.child_by_name(path).is_none() {
            self.append_page(&self.new_page(path));
            self.pages_changed();
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
        let pages = self.pages();
        let Some(position) = pages.iter().position(|other| *other == page) else {
            return;
        };

        for sound in page.sounds().filter(Sound::playing) {
            let path = sound.path().to_variant();
            let _ = WidgetExt::activate_action(self, "app.stop-sound", Some(&path));
        }
        let neighbor = pages
            .get(position + 1)
            .or_else(|| pages.get(position.checked_sub(1)?));
        let title = self.title_of(&page);
        tabs.remove(&page);
        if let Some(neighbor) = neighbor {
            tabs.set_visible_child(neighbor);
        }
        self.pages_changed();

        // Translators: {} is the name of a tab.
        let toast = adw::Toast::new(&gettext("Removed “{}”").replace("{}", &title));
        toast.set_button_label(Some(&gettext("Undo")));
        let path = page.path().to_owned();
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.restore_folder(&page, position)
        ));
        // The name of the tab is only forgotten once it cannot come back.
        toast.connect_dismissed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if window.imp().tabs.child_by_name(&path).is_none() {
                    window.set_folder_name(&path, None);
                }
            }
        ));
        self.imp().toasts.add_toast(toast);
    }

    fn restore_folder(&self, page: &FolderPage, position: usize) {
        let tabs = &self.imp().tabs;
        if tabs.child_by_name(page.path()).is_none() {
            let mut pages = self.pages();
            pages.insert(position.min(pages.len()), page.clone());
            self.set_pages(&pages);
        }
        tabs.set_visible_child(page);
    }

    /// Moves the tab being shown one place. The pages are the same objects,
    /// so nothing is scanned again and nothing stops.
    fn move_folder(&self, right: bool) {
        let tabs = &self.imp().tabs;
        let Some(page) = tabs.visible_child().and_downcast::<FolderPage>() else {
            return;
        };
        let mut pages = self.pages();
        let position = pages.iter().position(|other| *other == page);
        let target = position.and_then(|position| {
            let target = if right {
                position + 1
            } else {
                position.checked_sub(1)?
            };
            (target < pages.len()).then_some((position, target))
        });
        if let Some((position, target)) = target {
            pages.swap(position, target);
            self.set_pages(&pages);
            tabs.set_visible_child(&page);
        }
    }

    /// Puts the folder tabs in the given order. The stack can only append,
    /// so the pages from the first difference on are added again.
    fn set_pages(&self, pages: &[FolderPage]) {
        let tabs = &self.imp().tabs;
        let current = self.pages();
        let same = current
            .iter()
            .zip(pages)
            .take_while(|(a, b)| a == b)
            .count();
        for page in &current[same..] {
            tabs.remove(page);
        }
        for page in &pages[same..] {
            self.append_page(page);
        }
        self.pages_changed();
    }

    /// Adds a folder tab at the end, before the "Favorites" tab.
    fn append_page(&self, page: &FolderPage) {
        let imp = self.imp();
        let favorites = imp.favorites.get().unwrap();
        if favorites.parent().is_some() {
            imp.tabs.remove(favorites);
        }
        imp.tabs
            .add_titled(page, Some(page.path()), &self.title_of(page));
        imp.tabs
            .add_titled(favorites, Some(FAVORITES), &gettext("Favorites"));
        self.update_favorites();
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

    /// After a tab was added, removed, or moved: the setting, the list that
    /// the favorites and the results read, and what depends on them.
    fn pages_changed(&self) {
        let imp = self.imp();
        let pages = self.pages();
        let directories: Vec<_> = pages.iter().map(|page| page.path().to_owned()).collect();
        if directories != self.directories() {
            self.set_directories(directories);
        }
        let stores: Vec<_> = pages.iter().map(|page| page.store().clone()).collect();
        let folders = imp.folders.get().unwrap();
        folders.splice(0, folders.n_items(), &stores);

        let name = if pages.is_empty() { "empty" } else { "library" };
        imp.content.set_visible_child_name(name);
        self.update_favorites();
        self.update_actions();
    }

    /// The "Favorites" tab only exists while there is a favorite.
    fn update_favorites(&self) {
        let imp = self.imp();
        let (Some(favorites), Some(model)) = (imp.favorites.get(), imp.favorites_model.get())
        else {
            return;
        };
        if favorites.parent().is_none() {
            return;
        }
        let any = model.n_items() > 0;
        imp.tabs.page(favorites).set_visible(any);
        if !any && imp.tabs.visible_child().as_ref() == Some(favorites.upcast_ref()) {
            if let Some(first) = self.pages().first() {
                imp.tabs.set_visible_child(first);
            }
        }
    }

    /// The tab actions are for a folder tab: not for "Favorites", and not
    /// while the results are shown.
    fn update_actions(&self) {
        let pages = self.pages();
        let shown = self.imp().tabs.visible_child().and_downcast::<FolderPage>();
        let shown = shown.filter(|_| !self.searching());
        let position = shown.and_then(|page| pages.iter().position(|other| *other == page));
        let set = |name: &str, enabled: bool| {
            if let Some(action) = self.lookup_action(name) {
                if let Some(action) = action.downcast_ref::<gio::SimpleAction>() {
                    action.set_enabled(enabled);
                }
            }
        };
        set("remove-folder", position.is_some());
        set("rename-folder", position.is_some());
        set(
            "move-folder-left",
            position.is_some_and(|position| position > 0),
        );
        set(
            "move-folder-right",
            position.is_some_and(|position| position + 1 < pages.len()),
        );
    }

    /// The title of a tab without a name given by the user.
    fn default_title(&self, page: &FolderPage) -> String {
        let app = app();
        if app.is_some_and(|app| app.sounds_folder() == Path::new(page.path())) {
            gettext("Sounds")
        } else {
            page.folder_name()
        }
    }

    fn title_of(&self, page: &FolderPage) -> String {
        let name = self.folder_names().remove(page.path());
        name.unwrap_or_else(|| self.default_title(page))
    }

    fn update_titles(&self) {
        for page in self.pages() {
            let title = self.title_of(&page);
            self.imp().tabs.page(&page).set_title(Some(&title));
        }
    }

    /// Asks for the label of the tab being shown. The folder on disk keeps
    /// its name.
    async fn rename_folder(&self) {
        let page = self.imp().tabs.visible_child().and_downcast::<FolderPage>();
        let Some(page) = page.filter(|_| !self.searching()) else {
            return;
        };
        let builder = gtk::Builder::from_resource("/io/github/wilfison/Vinheta/rename-dialog.ui");
        let dialog: adw::AlertDialog = builder.object("dialog").unwrap();
        let entry: gtk::Entry = builder.object("entry").unwrap();
        entry.set_text(&self.title_of(&page));
        if dialog.choose_future(self).await != "rename" {
            return;
        }
        let name = entry.text().trim().to_owned();
        let custom = !name.is_empty() && name != self.default_title(&page);
        self.set_folder_name(page.path(), custom.then_some(&name));
    }

    fn folder_names(&self) -> HashMap<String, String> {
        let settings = self.imp().settings.get().unwrap();
        settings.value("folder-names").get().unwrap_or_default()
    }

    fn set_folder_name(&self, path: &str, name: Option<&str>) {
        let mut names = self.folder_names();
        let old = match name {
            Some(name) => names.insert(path.to_owned(), name.to_owned()),
            None => names.remove(path),
        };
        if old.as_deref() == name {
            return;
        }
        let settings = self.imp().settings.get().unwrap();
        if let Err(error) = settings.set("folder-names", names) {
            glib::g_warning!("vinheta", "could not save the tab names: {error}");
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
