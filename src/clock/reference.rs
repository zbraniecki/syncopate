use std::time::Duration;

use super::{Clock, MonoInstant, WallInstant};

pub struct TimeReference {
    mono_ref: MonoInstant,
    wall_ref: WallInstant,
}

impl TimeReference {
    pub fn new(mono_ref: MonoInstant, wall_ref: WallInstant) -> Self {
        Self { mono_ref, wall_ref }
    }

    pub fn capture(clock: &dyn Clock) -> Self {
        Self {
            mono_ref: clock.monotonic_now(),
            wall_ref: clock.wall_now(),
        }
    }

    pub fn wall_to_mono(&self, wall: WallInstant) -> MonoInstant {
        match wall.checked_duration_since(self.wall_ref) {
            Some(d) => self.mono_ref + d,
            None => {
                let d = self
                    .wall_ref
                    .checked_duration_since(wall)
                    .unwrap_or(Duration::ZERO);
                self.mono_ref - d
            }
        }
    }

    pub fn mono_to_wall(&self, mono: MonoInstant) -> WallInstant {
        match mono.checked_duration_since(self.mono_ref) {
            Some(d) => self.wall_ref + d,
            None => {
                let d = self
                    .mono_ref
                    .checked_duration_since(mono)
                    .unwrap_or(Duration::ZERO);
                self.wall_ref - d
            }
        }
    }
}
