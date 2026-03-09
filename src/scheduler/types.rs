use crate::clock::{MonoInstant, WallInstant};
use crate::task::{Drift, Task};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddTaskError {
    #[error("Deadline is in the past")]
    DeadlineInPast,
    #[error("Clock went backward")]
    ClockWentBackward,
}

#[derive(Debug)]
pub struct TaskExecution<'a, Ctx = ()> {
    pub task: &'a Task<Ctx>,
    pub drift: Drift,
}

#[derive(Debug)]
pub struct MissedExecution<'a, Ctx = ()> {
    pub task: &'a Task<Ctx>,
    pub deadlines_missed: Vec<Duration>,
}

#[derive(Debug)]
pub struct TickResult<'a, Ctx = ()> {
    pub fired: Vec<TaskExecution<'a, Ctx>>,
    pub missed: Vec<MissedExecution<'a, Ctx>>,
}

pub(crate) struct TaskState {
    pub(crate) added_at: MonoInstant,
    pub(crate) last_fired: Option<MonoInstant>,
    pub(crate) last_wall_deadline: Option<WallInstant>,
    pub(crate) remaining: Option<u32>,
}

pub(crate) struct ScheduledTask<Ctx = ()> {
    pub(crate) task: Task<Ctx>,
    pub(crate) state: TaskState,
}
