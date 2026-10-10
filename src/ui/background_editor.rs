/* background_editor.rs
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
use std::sync::OnceLock;

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};
use vinheta::backgrounds::{self, PAD_ASPECT};
use vinheta::pads::Crop;

/// The frame has the aspect of a pad, about twice its size.
const FRAME_WIDTH: f32 = 294.0;
const FRAME_HEIGHT: f32 = FRAME_WIDTH / PAD_ASPECT as f32;
const MARGIN: f32 = 12.0;
const RADIUS: f32 = 12.0;
/// What a key press moves, in fractions of the crop.
const STEP: f64 = 0.02;
const BIG_STEP: f64 = 0.1;
const KEY_ZOOM: f64 = 0.25;
const WHEEL_ZOOM: f64 = 1.1;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BackgroundEditor {
        pub texture: RefCell<Option<gdk::Texture>>,
        pub crop: Cell<Option<Crop>>,
        /// The crop a drag started from.
        pub dragged: Cell<Option<Crop>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BackgroundEditor {
        const NAME: &'static str = "VinhetaBackgroundEditor";
        type Type = super::BackgroundEditor;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("background-editor");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    impl ObjectImpl for BackgroundEditor {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("changed").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.set_halign(gtk::Align::Center);
            obj.update_property(&[
                gtk::accessible::Property::Label(&gettext("Background preview")),
                gtk::accessible::Property::Description(&gettext(
                    "Drag the image to move it. The arrow keys move it, plus and minus zoom it.",
                )),
            ]);
            obj.setup_controllers();
        }
    }

    impl WidgetImpl for BackgroundEditor {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let size = match orientation {
                gtk::Orientation::Horizontal => FRAME_WIDTH + 2.0 * MARGIN,
                _ => FRAME_HEIGHT + 2.0 * MARGIN,
            };
            let size = size.round() as i32;
            (size, size, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(texture) = self.texture.borrow().clone() else {
                return;
            };
            let (width, height) = (obj.width() as f32, obj.height() as f32);
            let frame = obj.frame();
            let (x, y, drawn_width, drawn_height) = backgrounds::cover(
                obj.image_size(),
                self.crop.get(),
                (f64::from(frame.width()), f64::from(frame.height())),
            );
            snapshot.append_scaled_texture(
                &texture,
                gsk::ScalingFilter::Trilinear,
                &graphene::Rect::new(
                    frame.x() + x as f32,
                    frame.y() + y as f32,
                    drawn_width as f32,
                    drawn_height as f32,
                ),
            );

            // What the pad does not show is dimmed.
            let dim = gdk::RGBA::new(0.0, 0.0, 0.0, 0.55);
            let (left, top) = (frame.x(), frame.y());
            let (right, bottom) = (left + frame.width(), top + frame.height());
            for area in [
                graphene::Rect::new(0.0, 0.0, width, top),
                graphene::Rect::new(0.0, bottom, width, height - bottom),
                graphene::Rect::new(0.0, top, left, frame.height()),
                graphene::Rect::new(right, top, width - right, frame.height()),
            ] {
                snapshot.append_color(&dim, &area);
            }
            let outline = gsk::RoundedRect::from_rect(frame, RADIUS);
            let white = gdk::RGBA::WHITE;
            snapshot.append_border(&outline, &[2.0; 4], &[white, white, white, white]);
        }
    }
}

glib::wrapper! {
    /// A picture behind a frame of the aspect of a pad: dragging, the wheel,
    /// and the keys choose the area of the picture inside the frame.
    pub struct BackgroundEditor(ObjectSubclass<imp::BackgroundEditor>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for BackgroundEditor {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl BackgroundEditor {
    /// Shows a picture with a crop, or fitted when there is none.
    pub fn set_picture(&self, texture: &gdk::Texture, crop: Option<Crop>) {
        let imp = self.imp();
        imp.texture.replace(Some(texture.clone()));
        let fit = Crop::fit(self.image_size(), PAD_ASPECT);
        self.set_crop(crop.unwrap_or(fit));
    }

    pub fn crop(&self) -> Crop {
        self.imp().crop.get().unwrap_or(Crop::WHOLE)
    }

    pub fn zoom(&self) -> f64 {
        self.crop().zoom(self.image_size(), PAD_ASPECT)
    }

    pub fn set_zoom(&self, zoom: f64) {
        let crop = self.crop().zoomed(self.image_size(), PAD_ASPECT, zoom);
        self.set_crop(crop);
    }

    pub fn connect_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("changed", false, move |values| {
            let editor = values[0].get::<Self>().unwrap();
            f(&editor);
            None
        })
    }

    fn set_crop(&self, crop: Crop) {
        if self.imp().crop.replace(Some(crop)) != Some(crop) {
            self.queue_draw();
            self.emit_by_name::<()>("changed", &[]);
        }
    }

    fn image_size(&self) -> (f64, f64) {
        let texture = self.imp().texture.borrow();
        texture.as_ref().map_or((1.0, 1.0), |texture| {
            (f64::from(texture.width()), f64::from(texture.height()))
        })
    }

    /// The frame, centered in the widget.
    fn frame(&self) -> graphene::Rect {
        let (width, height) = (self.width() as f32, self.height() as f32);
        graphene::Rect::new(
            ((width - FRAME_WIDTH) / 2.0).round(),
            ((height - FRAME_HEIGHT) / 2.0).round(),
            FRAME_WIDTH,
            FRAME_HEIGHT,
        )
    }

    /// Moves the crop by pixels of the frame from where it was.
    fn drag_from(&self, from: Crop, dx: f64, dy: f64) {
        let frame = self.frame();
        let (_, _, width, height) = backgrounds::cover(
            self.image_size(),
            Some(from),
            (f64::from(frame.width()), f64::from(frame.height())),
        );
        // The picture follows the pointer, so the crop goes the other way.
        self.set_crop(from.moved(-dx / width, -dy / height));
    }

    fn setup_controllers(&self) {
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to = editor)]
            self,
            move |_, _, _| {
                editor.grab_focus();
                editor.imp().dragged.set(Some(editor.crop()));
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to = editor)]
            self,
            move |_, dx, dy| {
                if let Some(from) = editor.imp().dragged.get() {
                    editor.drag_from(from, dx, dy);
                }
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak(rename_to = editor)]
            self,
            move |_, _, _| editor.imp().dragged.set(None)
        ));
        self.add_controller(drag);

        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to = editor)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _, dy| {
                let zoom = editor.zoom() * WHEEL_ZOOM.powf(-dy);
                editor.set_zoom(zoom);
                glib::Propagation::Stop
            }
        ));
        self.add_controller(scroll);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = editor)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, keyval, _, state| editor.key_pressed(keyval, state)
        ));
        self.add_controller(keys);
    }

    fn key_pressed(&self, keyval: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        let crop = self.crop();
        let step = if state.contains(gdk::ModifierType::SHIFT_MASK) {
            BIG_STEP
        } else {
            STEP
        };
        let (dx, dy) = (crop.width * step, crop.height * step);
        match keyval {
            gdk::Key::Left | gdk::Key::KP_Left => self.set_crop(crop.moved(-dx, 0.0)),
            gdk::Key::Right | gdk::Key::KP_Right => self.set_crop(crop.moved(dx, 0.0)),
            gdk::Key::Up | gdk::Key::KP_Up => self.set_crop(crop.moved(0.0, -dy)),
            gdk::Key::Down | gdk::Key::KP_Down => self.set_crop(crop.moved(0.0, dy)),
            gdk::Key::plus | gdk::Key::KP_Add | gdk::Key::equal => {
                self.set_zoom(self.zoom() + KEY_ZOOM);
            }
            gdk::Key::minus | gdk::Key::KP_Subtract => self.set_zoom(self.zoom() - KEY_ZOOM),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }
}
