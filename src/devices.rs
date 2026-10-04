/* devices.rs
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

//! What a device selector shows. No GTK types, so it can be tested.

use crate::audio::Device;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Node name stored in the settings. Empty for the system default.
    pub name: String,
    pub description: String,
    /// False for a chosen device that is not connected.
    pub available: bool,
}

/// The entries of a selector and the position of the selected one: the
/// system default, the devices, and the chosen device when it is not among
/// them. `last_description` is how that device was described when last seen.
pub fn selector_entries(
    devices: &[Device],
    chosen: &str,
    last_description: Option<&str>,
) -> (Vec<Entry>, usize) {
    let mut entries = vec![Entry {
        name: String::new(),
        description: String::new(),
        available: true,
    }];
    entries.extend(devices.iter().map(|device| Entry {
        name: device.name.clone(),
        description: device.description.clone(),
        available: true,
    }));
    if chosen.is_empty() {
        return (entries, 0);
    }
    let selected = entries.iter().position(|entry| entry.name == chosen);
    let selected = selected.unwrap_or_else(|| {
        entries.push(Entry {
            name: chosen.to_owned(),
            description: last_description.unwrap_or(chosen).to_owned(),
            available: false,
        });
        entries.len() - 1
    });
    (entries, selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Vec<Device> {
        [
            ("usb", "USB Microphone"),
            ("internal", "Internal Microphone"),
        ]
        .map(|(name, description)| Device {
            name: name.into(),
            description: description.into(),
        })
        .to_vec()
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn the_system_default_comes_first() {
        let (entries, selected) = selector_entries(&[], "", None);
        assert_eq!(names(&entries), [""]);
        assert_eq!(selected, 0);
        assert!(entries[0].available);
    }

    #[test]
    fn devices_keep_their_order() {
        let (entries, selected) = selector_entries(&devices(), "", None);
        assert_eq!(names(&entries), ["", "usb", "internal"]);
        assert_eq!(selected, 0);
        assert!(entries.iter().all(|entry| entry.available));
    }

    #[test]
    fn the_chosen_device_is_selected() {
        let (entries, selected) = selector_entries(&devices(), "internal", None);
        assert_eq!(entries.len(), 3);
        assert_eq!(selected, 2);
    }

    #[test]
    fn a_missing_chosen_device_is_added_as_unavailable() {
        let (entries, selected) = selector_entries(&devices(), "gone", None);
        assert_eq!(names(&entries), ["", "usb", "internal", "gone"]);
        assert_eq!(selected, 3);
        assert!(!entries[3].available);
        assert_eq!(entries[3].description, "gone");
    }

    #[test]
    fn a_missing_device_keeps_its_last_description() {
        let (entries, selected) = selector_entries(&[], "gone", Some("Headset"));
        assert_eq!(entries[selected].description, "Headset");
        assert!(!entries[selected].available);
    }
}
