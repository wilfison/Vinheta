/* limiter.rs
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

use std::time::Duration;

/// A gain limiter that works per buffer: the gain drops at once to what
/// keeps the loudest sample of a buffer at the ceiling, and comes back up
/// sample by sample. It scales samples and never bends them, so it adds no
/// harmonics.
pub(super) struct Limiter {
    ceiling: f32,
    /// The share of the distance to the target covered on each sample of a
    /// release.
    release: f32,
    gain: f32,
}

impl Limiter {
    pub(super) fn new(ceiling: f32, release: Duration, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let seconds = release.as_secs_f32().max(1.0 / rate);
        Self {
            ceiling: ceiling.clamp(f32::MIN_POSITIVE, 1.0),
            release: 1.0 - (-1.0 / (seconds * rate)).exp(),
            gain: 1.0,
        }
    }

    pub(super) fn process(&mut self, samples: &mut [f32]) {
        let peak = samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        if target <= self.gain {
            self.gain = target;
            for sample in samples {
                *sample *= target;
            }
        } else {
            for sample in samples {
                self.gain += (target - self.gain) * self.release;
                *sample *= self.gain;
            }
        }
    }

    pub(super) fn reset(&mut self) {
        self.gain = 1.0;
    }

    #[cfg(test)]
    pub(super) fn gain(&self) -> f32 {
        self.gain
    }
}

#[cfg(test)]
mod tests;
