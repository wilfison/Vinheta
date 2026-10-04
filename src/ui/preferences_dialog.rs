/* preferences_dialog.rs
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

use std::path::Path;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use vinheta::editors;
use vinheta::pads::TriggerMode;

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
        #[template_child]
        pub trigger_mode: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub fade_out: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub sounds_folder: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub open_folder: TemplateChild<gtk::Button>,
        #[template_child]
        pub choose_folder: TemplateChild<gtk::Button>,
        #[template_child]
        pub audio_editor: TemplateChild<adw::ComboRow>,
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
            settings
                .bind("fade-out-on-stop", &*self.fade_out, "active")
                .build();

            // The entries of the row are in the order of `TriggerMode::ALL`.
            self.trigger_mode.connect_selected_notify(|row| {
                let mode = TriggerMode::ALL.get(row.selected() as usize);
                row.set_subtitle(&match mode.copied().unwrap_or_default() {
                    TriggerMode::Overlap => {
                        gettext("Sounds play together; a click on a playing pad stops it")
                    }
                    TriggerMode::Restart => gettext("A click on a playing pad starts it again"),
                    TriggerMode::StopOthers => gettext("Starting a pad stops every other sound"),
                });
            });
            settings
                .bind("trigger-mode", &*self.trigger_mode, "selected")
                .mapping(|value, _| {
                    let mode = TriggerMode::from_name(value.str()?);
                    let position = TriggerMode::ALL.iter().position(|other| *other == mode)?;
                    Some((position as u32).to_value())
                })
                .set_mapping(|value, _| {
                    let position = value.get::<u32>().ok()? as usize;
                    Some(TriggerMode::ALL.get(position)?.name().to_variant())
                })
                .build();
            self.trigger_mode.notify("selected");
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

        // A chosen device that is not connected is told in the subtitle,
        // which is never ellipsized.
        let rows = [
            (imp.microphone.get(), DeviceKind::Microphone),
            (imp.monitor_output.get(), DeviceKind::Output),
        ]
        .map(|(row, kind)| (row.subtitle(), row, kind));
        let update = glib::clone!(
            #[weak]
            app,
            move || {
                for (subtitle, row, kind) in &rows {
                    if app.device_missing(*kind) {
                        row.set_subtitle(&gettext("Not connected. Using the system default."));
                        row.add_css_class("warning");
                    } else {
                        row.set_subtitle(subtitle.as_deref().unwrap_or_default());
                        row.remove_css_class("warning");
                    }
                }
            }
        );
        update();
        let handler = app.connect_local("devices-changed", false, move |_| {
            update();
            None
        });
        let handler = std::cell::RefCell::new(Some(handler));
        dialog.connect_closed(glib::clone!(
            #[weak]
            app,
            move |_| {
                if let Some(handler) = handler.take() {
                    app.disconnect(handler);
                }
            }
        ));

        // The row follows the key while the dialog is open.
        let settings = gio::Settings::new(APP_ID);
        settings.connect_changed(
            Some("sounds-folder"),
            glib::clone!(
                #[weak]
                dialog,
                #[weak]
                app,
                move |_, _| dialog.show_sounds_folder(&app)
            ),
        );
        dialog.show_sounds_folder(app);
        imp.open_folder.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            #[weak]
            app,
            move |_| {
                glib::spawn_future_local(async move { dialog.open_sounds_folder(&app).await });
            }
        ));
        imp.choose_folder.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            move |_| {
                let settings = settings.clone();
                glib::spawn_future_local(async move {
                    dialog.choose_sounds_folder(&settings).await;
                });
            }
        ));
        dialog.setup_audio_editor(app);
        dialog
    }

    /// The "Audio Editor" row: "Automatic", then the installed apps that
    /// open sounds. It is the only place that writes `audio-editor`.
    fn setup_audio_editor(&self, app: &VinhetaApplication) {
        let row = self.imp().audio_editor.get();
        let settings = gio::Settings::new(APP_ID);
        let apps: Vec<_> = app.audio_apps().into_iter().map(|(app, _)| app).collect();
        let (entries, selected) =
            editors::selector_entries(&apps, &settings.string("audio-editor"));
        let automatic = gettext("Automatic");
        let labels: Vec<&str> = std::iter::once(automatic.as_str())
            .chain(entries.iter().map(|app| app.name.as_str()))
            .collect();
        row.set_model(Some(&gtk::StringList::new(&labels)));
        row.set_selected(selected as u32);

        // What "Automatic" opens.
        let detected = editors::pick(&apps, "").map(|app| app.name.clone());
        let show = move |row: &adw::ComboRow| {
            let subtitle = match (row.selected(), &detected) {
                // Translators: {} is the name of an app, as in "Uses Audacity".
                (0, Some(name)) => gettext("Uses {}").replace("{}", name),
                (0, None) => gettext("No audio editor was found"),
                _ => String::new(),
            };
            row.set_subtitle(&glib::markup_escape_text(&subtitle));
        };
        show(&row);

        let ids: Vec<String> = entries.into_iter().map(|app| app.id).collect();
        row.connect_selected_notify({
            let settings = settings.clone();
            let ids = ids.clone();
            move |row| {
                show(row);
                let id = match row.selected() as usize {
                    0 => "",
                    position => ids.get(position - 1).map_or("", String::as_str),
                };
                if settings.string("audio-editor") != id {
                    if let Err(error) = settings.set_string("audio-editor", id) {
                        glib::g_warning!("vinheta", "could not save the audio editor: {error}");
                    }
                }
            }
        });
        // The row follows the key while the dialog is open. An app that is
        // not installed is shown as "Automatic", which is what it does.
        settings.connect_changed(
            Some("audio-editor"),
            glib::clone!(
                #[weak]
                row,
                move |settings, key| {
                    let chosen = settings.string(key);
                    let position = ids.iter().position(|id| *id == chosen.as_str());
                    row.set_selected(position.map_or(0, |position| position as u32 + 1));
                }
            ),
        );
    }

    /// The path, with the home directory as `~`.
    fn show_sounds_folder(&self, app: &VinhetaApplication) {
        let folder = app.sounds_folder();
        let text = match folder.strip_prefix(glib::home_dir()) {
            Ok(rest) => Path::new("~").join(rest),
            Err(_) => folder,
        };
        self.imp()
            .sounds_folder
            .set_subtitle(&glib::markup_escape_text(&text.to_string_lossy()));
    }

    fn window(&self) -> Option<gtk::Window> {
        self.root().and_downcast()
    }

    /// Shows the folder in the file manager. It only exists after the first
    /// import, so it is created here when needed.
    async fn open_sounds_folder(&self, app: &VinhetaApplication) {
        let folder = app.sounds_folder();
        let result = match std::fs::create_dir_all(&folder) {
            Ok(()) => {
                let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&folder)));
                let launched = launcher.launch_future(self.window().as_ref()).await;
                launched.map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
            glib::g_warning!("vinheta", "could not open {}: {error}", folder.display());
            self.add_toast(adw::Toast::new(&gettext("Could not open the folder")));
        }
    }

    /// Only changes where the next imports go: nothing is moved.
    async fn choose_sounds_folder(&self, settings: &gio::Settings) {
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Sounds Folder"))
            .modal(true)
            .build();
        let Ok(folder) = chooser.select_folder_future(self.window().as_ref()).await else {
            return;
        };
        let path = folder.path();
        if let Some(path) = path.as_deref().and_then(|path| path.to_str()) {
            if let Err(error) = settings.set_string("sounds-folder", path) {
                glib::g_warning!("vinheta", "could not save the sounds folder: {error}");
            }
        }
    }
}
