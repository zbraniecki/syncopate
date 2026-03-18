mod builder;
mod drift;
mod schedule;

pub use builder::{Absolute, Relative, TaskBuilder};
pub use drift::Drift;
pub use schedule::{MissedTickBehavior, PeriodicSchedule, Repeat, Window};

use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct RelativeTask {
    pub period: Duration,
    pub window: Option<Window>,
    pub schedule: PeriodicSchedule,
    pub on_miss: MissedTickBehavior,
    pub initial_delay: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AbsoluteTask {
    pub period: Duration,
    pub offset: Option<Duration>,
    pub window: Option<Window>,
    pub on_miss: MissedTickBehavior,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    Relative(RelativeTask),
    Absolute(AbsoluteTask),
}

pub type TaskCallback<Ctx> = fn(&Ctx, Drift);
pub type MissCallback<Ctx> = fn(&Ctx, &[Duration]);

#[derive(Debug)]
pub struct Task<Ctx = ()> {
    pub task_type: TaskType,
    pub repeat: Repeat,
    pub priority: u8,
    pub name: Option<String>,
    pub on_execute: Option<TaskCallback<Ctx>>,
    pub on_missed: Option<MissCallback<Ctx>>,
}
