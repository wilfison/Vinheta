/* devices.rs
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

/// Whether the chosen device is not among the devices. The system default
/// (an empty name) is never missing.
pub fn is_missing(devices: &[Device], chosen: &str) -> bool {
    !chosen.is_empty() && !devices.iter().any(|device| device.name == chosen)
}

#[cfg(test)]
mod tests;
