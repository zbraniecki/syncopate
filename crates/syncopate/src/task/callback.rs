use crate::task::TaskId;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Callback type for task execution.
/// Receives timing information when a task is executed and a reference to the user's context.
///
/// Note: For multi-threaded usage (with `SchedulerHandle`), callbacks must be `Send + Sync`.
/// For single-threaded usage (with `build_local()`), the `Send + Sync` bounds are not required.
pub type TaskCallback<Ctx> = Arc<dyn Fn(TaskExecution, &Ctx) + Send + Sync>;

/// Callback type for task misses.
/// Receives information when a task misses its execution window and a reference to the user's context.
///
/// Note: For multi-threaded usage (with `SchedulerHandle`), callbacks must be `Send + Sync`.
/// For single-threaded usage (with `build_local()`), the `Send + Sync` bounds are not required.
pub type MissCallback<Ctx> = Arc<dyn Fn(TaskMiss, &Ctx) + Send + Sync>;

/// Information provided to a task execution callback.
#[derive(Debug, Clone)]
pub struct TaskExecution {
    /// The task's unique identifier.
    pub task_id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// When the task was actually executed (polled).
    pub actual_time: Instant,

    /// Drift between ideal and actual execution time.
    pub drift: Duration,

    /// Whether the task was executed after its ideal time.
    pub was_late: bool,
}

/// Information provided to a task miss callback.
#[derive(Debug, Clone)]
pub struct TaskMiss {
    /// The task's unique identifier.
    pub task_id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// The end of the acceptable execution window that was missed.
    pub window_end: Instant,

    /// Number of consecutive misses for this task.
    pub miss_count: usize,
}
