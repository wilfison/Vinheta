/* folder_page.rs
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

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib};
use vinheta::library::{self, SoundFile};

use super::sound_grid::SoundGrid;
use crate::application::VinhetaApplication;
use crate::sound::Sound;

/// How long after a change in the folder it is scanned again. Changes come
/// in bursts.
const RESCAN_DELAY: Duration = Duration::from_millis(200);

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/folder-page.ui")]
    pub struct FolderPage {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub grid: TemplateChild<SoundGrid>,
        #[template_child]
        pub missing: TemplateChild<adw::StatusPage>,
        pub path: OnceCell<String>,
        /// The sounds of the folder, in no order: the views sort them.
        pub sounds: OnceCell<gio::ListStore>,
        pub monitor: RefCell<Option<gio::FileMonitor>>,
        /// Set while a rescan waits for more changes.
        pub rescan_timer: RefCell<Option<glib::SourceId>>,
        /// Counts the scans, so that only the newest one is applied.
        pub scan: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FolderPage {
        const NAME: &'static str = "VinhetaFolderPage";
        type Type = super::FolderPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            SoundGrid::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FolderPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.sounds.set(gio::ListStore::new::<Sound>()).unwrap();
        }

        fn dispose(&self) {
            if let Some(monitor) = self.monitor.take() {
                monitor.cancel();
            }
            if let Some(timer) = self.rescan_timer.take() {
                timer.remove();
            }
        }
    }

    impl WidgetImpl for FolderPage {}
    impl BinImpl for FolderPage {}
}

glib::wrapper! {
    /// The content of one tab: the pads of one folder, which it watches.
    pub struct FolderPage(ObjectSubclass<imp::FolderPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FolderPage {
    /// `sorter` is the order shared by every view.
    pub fn new(path: &str, sorter: &impl IsA<gtk::Sorter>) -> Self {
        let page: Self = glib::Object::new();
        let imp = page.imp();
        imp.path.set(path.to_owned()).unwrap();
        let sorted = gtk::SortListModel::new(Some(page.store().clone()), Some(sorter.clone()));
        imp.grid.set_model(&sorted);
        page.watch();
        page.rescan();
        page
    }

    pub fn path(&self) -> &str {
        self.imp().path.get().unwrap()
    }

    /// The name of the directory.
    pub fn folder_name(&self) -> String {
        Path::new(self.path())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path().to_owned())
    }

    /// The unsorted list of the sounds of the folder.
    pub fn store(&self) -> &gio::ListStore {
        self.imp().sounds.get().unwrap()
    }

    pub fn sounds(&self) -> impl Iterator<Item = Sound> {
        self.store()
            .iter::<Sound>()
            .flatten()
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Scrolls to the pad of a sound of this folder and blinks it.
    pub fn locate(&self, sound: &Sound) {
        self.imp().grid.locate(sound);
    }

    fn watch(&self) {
        let folder = gio::File::for_path(self.path());
        let flags = gio::FileMonitorFlags::WATCH_MOVES;
        match folder.monitor_directory(flags, gio::Cancellable::NONE) {
            Ok(monitor) => {
                monitor.connect_changed(glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |_, file, other, event| page.file_changed(file, other, event)
                ));
                self.imp().monitor.replace(Some(monitor));
            }
            Err(error) => glib::g_debug!("vinheta", "cannot watch {}: {error}", self.path()),
        }
    }

    /// The events are not a complete log, so they only ask for a new scan.
    /// The ones that carry both paths also move the pad settings.
    fn file_changed(
        &self,
        file: &gio::File,
        other: Option<&gio::File>,
        event: gio::FileMonitorEvent,
    ) {
        use gio::FileMonitorEvent::{MovedIn, MovedOut, Renamed};
        let path = |file: &gio::File| file.path()?.to_str().map(str::to_owned);
        let moved = match (event, path(file), other.and_then(path)) {
            (Renamed | MovedOut, Some(from), Some(to)) => Some((from, to)),
            (MovedIn, Some(to), Some(from)) => Some((from, to)),
            _ => None,
        };
        let app = gio::Application::default().and_downcast::<VinhetaApplication>();
        if let (Some(app), Some((from, to))) = (app, moved) {
            app.move_pad_settings(&from, &to);
        }

        let mut timer = self.imp().rescan_timer.borrow_mut();
        if timer.is_none() {
            *timer = Some(glib::timeout_add_local_once(
                RESCAN_DELAY,
                glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move || {
                        page.imp().rescan_timer.take();
                        page.rescan();
                    }
                ),
            ));
        }
    }

    /// Scans the folder off the main thread and shows what changed.
    pub fn rescan(&self) {
        let imp = self.imp();
        let scan = imp.scan.get().wrapping_add(1);
        imp.scan.set(scan);
        let folder = PathBuf::from(self.path());
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = page)]
            self,
            async move {
                let files = gio::spawn_blocking(move || library::scan(&folder)).await;
                if page.imp().scan.get() == scan {
                    page.show_files(files.ok().and_then(Result::ok));
                }
            }
        ));
    }

    /// `None` when the folder cannot be read. Sounds whose file is still
    /// there are kept, so a playing pad keeps playing.
    fn show_files(&self, files: Option<Vec<SoundFile>>) {
        let imp = self.imp();
        let store = self.store();
        // The application owns the pad settings.
        let app = gio::Application::default().and_downcast::<VinhetaApplication>();
        let current: Vec<_> = self
            .sounds()
            .map(|sound| PathBuf::from(sound.path()))
            .collect();
        let diff = library::diff(&current, files.as_deref().unwrap_or_default());

        for path in &diff.removed {
            let gone = self.sounds().find(|sound| Path::new(&sound.path()) == path);
            let Some(sound) = gone else { continue };
            if let Some(app) = &app {
                app.forget_sound(&sound);
            }
            if let Some(position) = store.find(&sound) {
                store.remove(position);
            }
        }
        // A file that was written again: its place in the "recent" order.
        let mut modified = false;
        for sound in self.sounds() {
            let file = files
                .iter()
                .flatten()
                .find(|file| file.path == Path::new(&sound.path()));
            if let Some(file) = file {
                modified |= sound.set_modified_time(file.modified);
            }
        }
        if let Some(app) = app.as_ref().filter(|_| modified) {
            app.sounds_modified();
        }
        // The actions take the path of a sound as a string.
        let added: Vec<_> = diff
            .added
            .iter()
            .filter_map(|file| {
                let path = file.path.to_str()?;
                let settings = app.as_ref().map(|app| app.pad_settings(path));
                let settings = settings.unwrap_or_default();
                let name = library::humanize(&file.name);
                Some(Sound::new(path, &name, file.modified, &settings))
            })
            .collect();
        store.extend_from_slice(&added);

        let child = match files {
            None => {
                imp.missing.set_description(Some(self.path()));
                "missing"
            }
            Some(_) if store.n_items() == 0 => "empty",
            Some(_) => "grid",
        };
        imp.stack.set_visible_child_name(child);
    }
}
