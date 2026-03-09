use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{Clock, MonoInstant, WallInstant};

pub struct RealClock {
    mono_epoch: Instant,
}

impl RealClock {
    pub fn new() -> Self {
        Self {
            mono_epoch: Instant::now(),
        }
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn monotonic_now(&self) -> MonoInstant {
        MonoInstant(self.mono_epoch.elapsed().as_nanos() as u64)
    }

    fn wall_now(&self) -> WallInstant {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as u64;
        WallInstant(ns)
    }
}
