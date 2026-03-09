use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{Clock, MonoInstant, WallInstant};

pub struct SimClock {
    mono_ns: AtomicU64,
    wall_ns: AtomicU64,
}

impl SimClock {
    pub fn new() -> Self {
        Self {
            mono_ns: AtomicU64::new(0),
            wall_ns: AtomicU64::new(0),
        }
    }

    pub fn advance(&self, duration: Duration) {
        let ns = duration.as_nanos() as u64;
        self.mono_ns.fetch_add(ns, Ordering::Relaxed);
        self.wall_ns.fetch_add(ns, Ordering::Relaxed);
    }

    pub fn jump_wall_clock(&self, delta_ns: i64) {
        let current = self.wall_ns.load(Ordering::Relaxed) as i64;
        let adjusted = current.saturating_add(delta_ns).max(0) as u64;
        self.wall_ns.store(adjusted, Ordering::Relaxed);
    }
}

impl Default for SimClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SimClock {
    fn monotonic_now(&self) -> MonoInstant {
        MonoInstant(self.mono_ns.load(Ordering::Relaxed))
    }

    fn wall_now(&self) -> WallInstant {
        WallInstant(self.wall_ns.load(Ordering::Relaxed))
    }
}
