use crate::scheduler::r#loop::{DueTask, MissedTask};
use std::time::{Duration, Instant};

/// The result of a scheduler poll. This is the primary interface between
/// the scheduler and the application.
#[derive(Debug)]
pub struct WakeupPlan {
    /// How long the application can sleep before the next task is due.
    pub idle_duration: Duration,

    /// The monotonic instant of the next wakeup, if any tasks are scheduled.
    pub next_wakeup: Option<Instant>,

    /// Tasks that are due now (their window includes the current time).
    pub due_tasks: Vec<DueTask>,

    /// Tasks that have missed their window entirely.
    pub missed_tasks: Vec<MissedTask>,
}
