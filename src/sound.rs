/* sound.rs
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

use std::cell::{Cell, OnceCell};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::Sound)]
    pub struct Sound {
        #[property(get, construct_only)]
        path: OnceCell<String>,
        #[property(get, construct_only)]
        name: OnceCell<String>,
        #[property(get, set)]
        playing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Sound {
        const NAME: &'static str = "VinhetaSound";
        type Type = super::Sound;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Sound {}
}

glib::wrapper! {
    /// One audio file of the library, shown as a pad.
    pub struct Sound(ObjectSubclass<imp::Sound>);
}

impl Sound {
    pub fn new(path: &str, name: &str) -> Self {
        glib::Object::builder()
            .property("path", path)
            .property("name", name)
            .build()
    }
}
