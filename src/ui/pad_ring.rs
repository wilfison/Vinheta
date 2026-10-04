/* pad_ring.rs
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

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, gsk};

/// The width of the border, drawn inside the pad so pads do not move.
const WIDTH: f32 = 3.0;
/// The corner radius of a card.
const RADIUS: f32 = 12.0;
/// The position is told every 100 ms: the border moves on by itself between
/// two of them, but no further than this, in milliseconds.
const AHEAD: i64 = 200;

/// A position of the playback and when it was told (monotonic, microseconds).
#[derive(Clone, Copy)]
pub struct Sample {
    elapsed: i64,
    duration: i64,
    at: i64,
}

impl Sample {
    fn timed(&self) -> bool {
        self.elapsed >= 0 && self.duration > 0
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PadRing {
        /// `None` while the sound does not play.
        pub sample: Cell<Option<Sample>>,
        pub tick: RefCell<Option<gtk::TickCallbackId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PadRing {
        const NAME: &'static str = "VinhetaPadRing";
        type Type = super::PadRing;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("ring");
        }
    }

    impl ObjectImpl for PadRing {
        fn dispose(&self) {
            if let Some(tick) = self.tick.take() {
                tick.remove();
            }
        }
    }

    impl WidgetImpl for PadRing {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(left) = obj.left() else { return };
            let (width, height) = (obj.width() as f32, obj.height() as f32);
            let inset = WIDTH / 2.0;
            let (start, top, end, bottom) = (inset, inset, width - inset, height - inset);
            let radius = (RADIUS - inset)
                .min((end - start) / 2.0)
                .min((bottom - top) / 2.0);
            if radius <= 0.0 {
                return;
            }

            // Clockwise from the middle of the top edge.
            let builder = gsk::PathBuilder::new();
            builder.move_to(width / 2.0, top);
            builder.line_to(end - radius, top);
            builder.arc_to(end, top, end, top + radius);
            builder.line_to(end, bottom - radius);
            builder.arc_to(end, bottom, end - radius, bottom);
            builder.line_to(start + radius, bottom);
            builder.arc_to(start, bottom, start, bottom - radius);
            builder.line_to(start, top + radius);
            builder.arc_to(start, top, start + radius, top);
            builder.line_to(width / 2.0, top);
            let mut path = builder.to_path();

            if left < 1.0 {
                let measure = gsk::PathMeasure::new(&path);
                let from = measure.point(measure.length() * (1.0 - left));
                let (Some(from), Some(to)) = (from, path.end_point()) else {
                    return;
                };
                let builder = gsk::PathBuilder::new();
                builder.add_segment(&path, &from, &to);
                path = builder.to_path();
            }
            snapshot.append_stroke(&path, &gsk::Stroke::new(WIDTH), &obj.color());
        }
    }
}

glib::wrapper! {
    /// The border of a pad that plays: it gets shorter as the sound goes on,
    /// and is whole while the duration is not known. Its color is the CSS
    /// `color` of the widget.
    pub struct PadRing(ObjectSubclass<imp::PadRing>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PadRing {
    /// Shows a position in milliseconds (negative while unknown), or nothing
    /// while the sound does not play.
    pub fn set_position(&self, position: Option<(i64, i64)>) {
        let imp = self.imp();
        let old = imp.sample.get();
        let sample = position.map(|(elapsed, duration)| match old {
            Some(old) if old.elapsed == elapsed && old.duration == duration => old,
            _ => Sample {
                elapsed,
                duration,
                at: glib::monotonic_time(),
            },
        });
        imp.sample.set(sample);

        let animate = sample.is_some_and(|sample| sample.timed())
            && self.settings().is_gtk_enable_animations();
        let mut tick = imp.tick.borrow_mut();
        if animate && tick.is_none() {
            tick.replace(self.add_tick_callback(|ring, _| {
                ring.queue_draw();
                glib::ControlFlow::Continue
            }));
        } else if !animate {
            if let Some(tick) = tick.take() {
                tick.remove();
            }
        }
        self.queue_draw();
    }

    /// The part of the border to draw, from 0 to 1.
    fn left(&self) -> Option<f32> {
        let imp = self.imp();
        let sample = imp.sample.get()?;
        if !sample.timed() {
            return Some(1.0);
        }
        let ahead = if imp.tick.borrow().is_some() {
            ((glib::monotonic_time() - sample.at) / 1000).clamp(0, AHEAD)
        } else {
            0
        };
        let elapsed = (sample.elapsed + ahead) as f32;
        Some((1.0 - elapsed / sample.duration as f32).clamp(0.0, 1.0))
    }
}
