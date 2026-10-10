/* sound_pad.rs
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
use std::time::Duration;

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};
use vinheta::backgrounds;
use vinheta::pads::{self, Crop, PadColor, PadSettings};

use crate::application::VinhetaApplication;
use crate::sound::Sound;
use crate::ui::pad_ring::PadRing;
use crate::VinhetaWindow;

/// How long the highlight of a located pad stays, the length of its animation.
const BLINK: Duration = Duration::from_millis(1200);
/// The corner radius of a card, which clips the background.
const RADIUS: f32 = 12.0;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/sound-pad.ui")]
    pub struct SoundPad {
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
        #[template_child]
        pub loop_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub favorite_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub shortcut: TemplateChild<gtk::Label>,
        #[template_child]
        pub times: TemplateChild<gtk::Box>,
        #[template_child]
        pub ring: TemplateChild<PadRing>,
        #[template_child]
        pub elapsed: TemplateChild<gtk::Label>,
        #[template_child]
        pub remaining: TemplateChild<gtk::Label>,
        #[template_child]
        pub menu: TemplateChild<gio::MenuModel>,
        pub sound: RefCell<Option<(Sound, glib::SignalHandlerId)>>,
        pub actions: gio::SimpleActionGroup,
        pub popover: RefCell<Option<gtk::PopoverMenu>>,
        /// Set while the pad is highlighted by `blink`.
        pub blink: RefCell<Option<glib::SourceId>>,
        /// The second last told to assistive technology, -1 for none.
        pub described: Cell<i64>,
        /// The file name of the background shown, and its picture.
        pub background: RefCell<(String, Option<gdk::Texture>)>,
        /// The area of the picture shown, `None` for all of it.
        pub crop: Cell<Option<Crop>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SoundPad {
        const NAME: &'static str = "VinhetaSoundPad";
        type Type = super::SoundPad;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            PadRing::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SoundPad {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_actions();

            // A secondary click does not activate the item of a grid view.
            let click = gtk::GestureClick::builder()
                .button(gdk::BUTTON_SECONDARY)
                .build();
            click.connect_pressed(glib::clone!(
                #[weak]
                obj,
                move |click, _, x, y| {
                    click.set_state(gtk::EventSequenceState::Claimed);
                    obj.open_menu(Some((x, y)));
                }
            ));
            obj.add_controller(click);

            let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
            long_press.connect_pressed(glib::clone!(
                #[weak]
                obj,
                move |long_press, x, y| {
                    long_press.set_state(gtk::EventSequenceState::Claimed);
                    obj.open_menu(Some((x, y)));
                }
            ));
            obj.add_controller(long_press);

            obj.setup_drop();
        }

        fn dispose(&self) {
            if let Some(timer) = self.blink.take() {
                timer.remove();
            }
            if let Some(popover) = self.popover.take() {
                popover.unparent();
            }
        }
    }

    impl WidgetImpl for SoundPad {
        /// The background goes under the children, over the card, and its
        /// crop covers the pad like `object-fit: cover`.
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(texture) = &self.background.borrow().1 {
                let obj = self.obj();
                let (width, height) = (obj.width() as f32, obj.height() as f32);
                let image = (f64::from(texture.width()), f64::from(texture.height()));
                let (x, y, drawn_width, drawn_height) = backgrounds::cover(
                    image,
                    self.crop.get(),
                    (f64::from(width), f64::from(height)),
                );
                let bounds = graphene::Rect::new(0.0, 0.0, width, height);
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(bounds, RADIUS));
                snapshot.append_scaled_texture(
                    texture,
                    gsk::ScalingFilter::Trilinear,
                    &graphene::Rect::new(
                        x as f32,
                        y as f32,
                        drawn_width as f32,
                        drawn_height as f32,
                    ),
                );
                snapshot.pop();
            }
            self.parent_snapshot(snapshot);
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if let Some(popover) = self.popover.borrow().as_ref() {
                popover.present();
            }
        }
    }

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
    /// Shows the given sound and follows its properties, or lets go of the
    /// current one. Pads are recycled, so nothing of the old sound may stay.
    pub fn set_sound(&self, sound: Option<&Sound>) {
        let imp = self.imp();
        if let Some((old, handler)) = imp.sound.take() {
            old.disconnect(handler);
        }
        if let Some(popover) = imp.popover.borrow().as_ref() {
            popover.popdown();
        }
        self.end_blink();
        let Some(sound) = sound else {
            imp.ring.set_position(None);
            self.show_background("", None);
            return;
        };
        imp.described.set(i64::MIN);

        let handler = sound.connect_notify_local(
            None,
            glib::clone!(
                #[weak(rename_to = pad)]
                self,
                move |sound, property| match property.name() {
                    "elapsed" | "duration" => pad.show_position(sound),
                    _ => pad.show_sound(sound),
                }
            ),
        );
        self.show_sound(sound);
        imp.sound.replace(Some((sound.clone(), handler)));
    }

    pub fn sound(&self) -> Option<Sound> {
        let sound = self.imp().sound.borrow();
        sound.as_ref().map(|(sound, _)| sound.clone())
    }

    /// Highlights the pad for a moment, to show where a sound is. The style
    /// pulses the outline twice.
    pub fn blink(&self) {
        self.end_blink();
        self.add_css_class("located");
        let timer = glib::timeout_add_local_once(
            BLINK,
            glib::clone!(
                #[weak(rename_to = pad)]
                self,
                move || {
                    pad.imp().blink.take();
                    pad.remove_css_class("located");
                }
            ),
        );
        self.imp().blink.replace(Some(timer));
    }

    fn end_blink(&self) {
        if let Some(timer) = self.imp().blink.take() {
            timer.remove();
        }
        self.remove_css_class("located");
    }

    fn show_sound(&self, sound: &Sound) {
        let imp = self.imp();
        imp.label.set_label(&sound.display_name());
        imp.loop_icon.set_visible(sound.looping());
        imp.favorite_icon.set_visible(sound.favorite());
        let key = sound.shortcut().chars().next().map(pads::shortcut_label);
        imp.shortcut.set_visible(key.is_some());
        imp.shortcut.set_label(key.as_deref().unwrap_or_default());
        let tooltip = match &key {
            // Translators: the first {} is the file name of a sound, the
            // second {} is the key that plays it, as in "Applause.wav, key Q".
            Some(key) => gettext("{}, key {}")
                .replacen("{}", &sound.file_name(), 1)
                .replacen("{}", key, 1),
            None => sound.file_name(),
        };
        self.set_tooltip_text(Some(&tooltip));
        // The description names the key.
        imp.described.set(i64::MIN);
        let color = sound.color();
        for other in PadColor::ALL {
            if other.name() == color {
                self.add_css_class(other.name());
            } else {
                self.remove_css_class(other.name());
            }
        }
        self.show_background(&sound.background(), sound.crop_area());
        if sound.playing() {
            self.add_css_class("playing");
        } else {
            self.remove_css_class("playing");
        }
        self.show_position(sound);

        let set = |name: &str, enabled: bool| {
            if let Some(action) = imp
                .actions
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(enabled);
            }
        };
        set("stop", sound.playing());
        set("reset", sound.settings() != PadSettings::default());
        imp.actions
            .change_action_state("loop", &sound.looping().to_variant());
        imp.actions
            .change_action_state("favorite", &sound.favorite().to_variant());
    }

    /// Loads the picture only when the file name changed: pads are recycled
    /// and `show_sound` runs on every change of the sound.
    fn show_background(&self, name: &str, crop: Option<Crop>) {
        let imp = self.imp();
        if imp.crop.replace(crop) != crop {
            self.queue_draw();
        }
        if imp.background.borrow().0 == name {
            return;
        }
        let app = gio::Application::default().and_downcast::<VinhetaApplication>();
        let texture = app.and_then(|app| app.background_texture(name));
        self.set_css_class("with-image", texture.is_some());
        imp.background.replace((name.to_owned(), texture));
        self.queue_draw();
    }

    fn set_css_class(&self, class: &str, on: bool) {
        if on {
            self.add_css_class(class);
        } else {
            self.remove_css_class(class);
        }
    }

    /// The times and the border only exist while the sound plays. Without a
    /// duration there is only the elapsed time, and the border stays whole.
    fn show_position(&self, sound: &Sound) {
        let imp = self.imp();
        let time = |millis: i64| Duration::from_millis(millis.max(0).unsigned_abs());
        let (elapsed, duration) = (sound.elapsed(), sound.duration());
        let known = sound.playing() && elapsed >= 0;
        imp.ring
            .set_position(sound.playing().then_some((elapsed, duration)));
        imp.times.set_visible(known);
        if known {
            imp.elapsed.set_label(&pads::format_time(time(elapsed)));
            if duration > 0 {
                let left = time(duration.saturating_sub(elapsed));
                imp.remaining.set_label(&pads::format_remaining(left));
            } else {
                imp.remaining.set_label("");
            }
        }

        // Assistive technology is told once per second at most.
        let second = match (sound.playing(), known) {
            (false, _) => -2,
            (true, false) => -1,
            (true, true) => elapsed / 1000,
        };
        if imp.described.replace(second) == second {
            return;
        }
        let key = sound.shortcut().chars().next().map(|key| {
            // Translators: {} is the key that plays a sound, as in "Key Q".
            gettext("Key {}").replace("{}", &pads::shortcut_label(key))
        });
        if !sound.playing() {
            match key {
                Some(key) => self.update_property(&[gtk::accessible::Property::Description(&key)]),
                None => self.reset_property(gtk::AccessibleProperty::Description),
            }
            return;
        }
        let description = if !known {
            gettext("Playing")
        } else if duration > 0 {
            // Translators: the first {} is the elapsed time of a sound, the
            // second {} is its duration, as in "Playing, 00:23 of 01:35".
            gettext("Playing, {} of {}")
                .replacen("{}", &pads::format_time(time(elapsed)), 1)
                .replacen("{}", &pads::format_time(time(duration)), 1)
        } else {
            // Translators: {} is the elapsed time of a sound, as in "Playing, 00:23".
            gettext("Playing, {}").replace("{}", &pads::format_time(time(elapsed)))
        };
        let description = match key {
            Some(key) => format!("{key}, {description}"),
            None => description,
        };
        self.update_property(&[gtk::accessible::Property::Description(&description)]);
    }

    fn window(&self) -> Option<VinhetaWindow> {
        self.root().and_upcast::<gtk::Widget>().and_downcast()
    }

    /// The first image dropped on a pad becomes its background. The pad takes
    /// every drop, so what is not an image goes where a drop on the window
    /// goes.
    fn setup_drop(&self) {
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = pad)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let (Ok(files), Some(sound)) = (value.get::<gdk::FileList>(), pad.sound()) else {
                    return false;
                };
                let paths = files.files().iter().filter_map(gio::File::path).collect();
                let (images, others) = backgrounds::split(paths);
                // Only one image can be the background; the others are left
                // out instead of being refused as sounds.
                if let Some(image) = images.iter().find_map(|path| path.to_str()) {
                    let parameter = (sound.path(), image.to_owned()).to_variant();
                    let _ = pad.activate_action("app.add-background", Some(&parameter));
                }
                let others: Vec<&str> = others.iter().filter_map(|path| path.to_str()).collect();
                if !others.is_empty() {
                    let _ = pad.activate_action("win.import-files", Some(&others.to_variant()));
                }
                if let Some(window) = pad.window() {
                    window.pad_dropped();
                }
                true
            }
        ));
        drop.connect_current_drop_notify(glib::clone!(
            #[weak(rename_to = pad)]
            self,
            move |drop| {
                let active = drop.current_drop().is_some();
                pad.set_css_class("drop-target", active);
                if let Some(window) = pad.window() {
                    window.pad_drop_changed(active);
                }
            }
        ));
        self.add_controller(drop);
    }

    /// The actions of the context menu. They go through the application and
    /// the window, which is where anything else changes a sound.
    fn setup_actions(&self) {
        let forward = |name: &'static str, target: &'static str| {
            gio::ActionEntry::builder(name)
                .activate(glib::clone!(
                    #[weak(rename_to = pad)]
                    self,
                    move |_: &gio::SimpleActionGroup, _, _| {
                        if let Some(sound) = pad.sound() {
                            let _ = pad.activate_action(target, Some(&sound.path().to_variant()));
                        }
                    }
                ))
                .build()
        };
        // Check items: the state follows the sound, in `show_sound`.
        let check = |name: &'static str, target: &'static str| {
            gio::ActionEntry::builder(name)
                .state(false.to_variant())
                .activate(glib::clone!(
                    #[weak(rename_to = pad)]
                    self,
                    move |_: &gio::SimpleActionGroup, _, _| {
                        if let Some(sound) = pad.sound() {
                            let _ = pad.activate_action(target, Some(&sound.path().to_variant()));
                        }
                    }
                ))
                .build()
        };
        let actions = &self.imp().actions;
        actions.add_action_entries([
            forward("edit", "win.edit-sound"),
            forward("open-editor", "app.open-in-editor"),
            forward("stop", "app.stop-sound"),
            forward("reset", "app.reset-sound"),
            forward("trash", "app.trash-sound"),
            check("loop", "app.toggle-loop"),
            check("favorite", "app.toggle-favorite"),
        ]);
        self.insert_action_group("pad", Some(actions));
    }

    /// Opens the context menu at a position of the pad, or at the pad itself.
    pub fn open_menu(&self, at: Option<(f64, f64)>) {
        let imp = self.imp();
        let Some(sound) = self.sound() else {
            return;
        };
        // Only the copies in the sounds folder can be trashed, and the
        // editor item needs an editor.
        let app = gio::Application::default().and_downcast::<VinhetaApplication>();
        let imported = app
            .as_ref()
            .is_some_and(|app| app.is_imported(&sound.path()));
        let editor = app.is_some_and(|app| app.audio_editor().is_some());
        for (name, enabled) in [("trash", imported), ("open-editor", editor)] {
            if let Some(action) = imp.actions.lookup_action(name) {
                if let Some(action) = action.downcast_ref::<gio::SimpleAction>() {
                    action.set_enabled(enabled);
                }
            }
        }
        if let Some(old) = imp.popover.take() {
            old.unparent();
        }
        let popover = gtk::PopoverMenu::from_model(Some(&*imp.menu));
        popover.set_parent(self);
        if let Some((x, y)) = at {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.set_has_arrow(false);
            popover.set_halign(gtk::Align::Start);
        }
        // The item that was chosen still needs the popover for a moment.
        popover.connect_closed(glib::clone!(
            #[weak(rename_to = pad)]
            self,
            move |popover| {
                let popover = popover.clone();
                glib::idle_add_local_once(move || {
                    let current = pad.imp().popover.borrow().clone();
                    if current.as_ref() == Some(&popover) {
                        pad.imp().popover.take();
                    }
                    if popover.parent().is_some() {
                        popover.unparent();
                    }
                });
            }
        ));
        imp.popover.replace(Some(popover.clone()));
        popover.popup();
    }
}
