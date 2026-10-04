/* sound_grid.rs
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

use std::cell::RefCell;
use std::time::Duration;

use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib};

use super::sound_pad::SoundPad;
use crate::sound::Sound;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/github/wilfison/Vinheta/sound-grid.ui")]
    pub struct SoundGrid {
        #[template_child]
        pub grid: TemplateChild<gtk::GridView>,
        /// The action a pad activates, with the path of its sound.
        pub action: RefCell<String>,
    }

    impl Default for SoundGrid {
        fn default() -> Self {
            Self {
                grid: TemplateChild::default(),
                action: RefCell::new("app.toggle-sound".to_owned()),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SoundGrid {
        const NAME: &'static str = "VinhetaSoundGrid";
        type Type = super::SoundGrid;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SoundGrid {
        fn constructed(&self) {
            self.parent_constructed();

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

            // Pads go through an action, the same entry point that anything
            // outside the window uses.
            let obj = self.obj();
            self.grid.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |grid, position| {
                    let sound = grid.model().and_then(|model| model.item(position));
                    if let Some(sound) = sound.and_downcast::<Sound>() {
                        let action = obj.imp().action.borrow().clone();
                        let _ = grid.activate_action(&action, Some(&sound.path().to_variant()));
                    }
                }
            ));

            // The focus is on the cell that holds the pad, so the keys that
            // open a context menu are handled here.
            let open_menu = gtk::CallbackAction::new(|grid, _| {
                let focus = grid.root().and_then(|root| root.focus());
                let pad = focus
                    .filter(|focus| focus.is_ancestor(grid))
                    .and_then(|focus| focus.first_child())
                    .and_downcast::<SoundPad>();
                match pad {
                    Some(pad) => {
                        pad.open_menu(None);
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Proceed,
                }
            });
            let keys = gtk::ShortcutController::new();
            keys.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("Menu|<Shift>F10"),
                Some(open_menu),
            ));
            self.grid.add_controller(keys);
        }
    }

    fn pad_and_sound(item: &glib::Object) -> Option<(SoundPad, Option<Sound>)> {
        let item = item.downcast_ref::<gtk::ListItem>()?;
        let pad = item.child().and_downcast()?;
        Some((pad, item.item().and_downcast()))
    }

    impl WidgetImpl for SoundGrid {}
    impl BinImpl for SoundGrid {}
}

glib::wrapper! {
    /// A grid of pads over any list of `Sound`, shared by the folder tabs,
    /// the favorites, and the search results.
    pub struct SoundGrid(ObjectSubclass<imp::SoundGrid>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for SoundGrid {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl SoundGrid {
    pub fn set_model(&self, model: &impl IsA<gio::ListModel>) {
        let selection = gtk::NoSelection::new(Some(model.clone()));
        self.imp().grid.set_model(Some(&selection));
    }

    /// The action a pad activates instead of `app.toggle-sound`.
    pub fn set_activate_action(&self, action: &str) {
        self.imp().action.replace(action.to_owned());
    }

    pub fn first_sound(&self) -> Option<Sound> {
        let model = self.imp().grid.model()?;
        model.item(0).and_downcast()
    }

    /// Scrolls to the pad of a sound and blinks it.
    pub fn locate(&self, sound: &Sound) {
        let grid = &self.imp().grid;
        let Some(model) = grid.model() else { return };
        let position = (0..model.n_items())
            .find(|position| model.item(*position).and_downcast_ref::<Sound>() == Some(sound));
        let Some(position) = position else { return };
        grid.scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
        // The pad of a cell that was off screen exists after the next layout.
        glib::timeout_add_local_once(
            Duration::from_millis(100),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                #[weak]
                sound,
                move || {
                    if let Some(pad) = this.pad_of(&sound) {
                        pad.blink();
                    }
                }
            ),
        );
    }

    fn pad_of(&self, sound: &Sound) -> Option<SoundPad> {
        let mut cell = self.imp().grid.first_child();
        while let Some(widget) = cell {
            let pad = widget.first_child().and_downcast::<SoundPad>();
            if let Some(pad) = pad.filter(|pad| pad.sound().as_ref() == Some(sound)) {
                return Some(pad);
            }
            cell = widget.next_sibling();
        }
        None
    }
}
