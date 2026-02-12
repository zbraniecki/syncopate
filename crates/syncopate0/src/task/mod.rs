mod callback;
mod config;

pub use callback::*;
pub use config::*;
use std::num::NonZeroU64;
use std::time::Duration;

/// Unique task identifier with generation counter.
/// High 32 bits: generation. Low 32 bits: arena index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(NonZeroU64);

impl TaskId {
    pub fn new(index: u32, generation: u32) -> Self {
        let id = ((generation as u64) << 32) | (index as u64);
        Self(NonZeroU64::new(id).expect("TaskId cannot be zero"))
    }

    pub fn index(&self) -> u32 {
        self.0.get() as u32
    }

    pub fn generation(&self) -> u32 {
        (self.0.get() >> 32) as u32
    }
}

/// The scheduling pattern for a task.
#[derive(Debug, Clone)]
pub enum TaskType {
    /// Recurring task with a fixed period.
    Periodic {
        /// Time between ideal execution points.
        period: Duration,

        /// Task may execute this much before its ideal time.
        window_before: Duration,

        /// Task may execute this much after its ideal time.
        window_after: Duration,

        /// If true, next deadline is wall-clock aligned (`next_deadline += period`).
        /// If false (interval mode), next deadline is relative to actual fire time
        /// (`next_deadline = now + period`). Interval mode is preferred for animations
        /// to avoid cascading misses from OS sleep overshoot.
        anchored: bool,
    },
}
