/* call_guide_dialog.rs
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

//! The call setup guide: what a call app needs, shown on the first run and
//! from the primary menu.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/call-guide-dialog.ui")]
    pub struct CallGuideDialog {
        #[template_child]
        pub got_it: TemplateChild<gtk::Button>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallGuideDialog {
        const NAME: &'static str = "VinhetaCallGuideDialog";
        type Type = super::CallGuideDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for CallGuideDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.got_it.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    obj.close();
                }
            ));
        }
    }

    impl WidgetImpl for CallGuideDialog {}
    impl AdwDialogImpl for CallGuideDialog {}
}

glib::wrapper! {
    pub struct CallGuideDialog(ObjectSubclass<imp::CallGuideDialog>)
        @extends gtk::Widget, adw::Dialog;
}

impl CallGuideDialog {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for CallGuideDialog {
    fn default() -> Self {
        Self::new()
    }
}
