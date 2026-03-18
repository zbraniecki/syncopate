mod builder;
mod drift;
mod schedule;

pub use builder::{Absolute, Relative, TaskBuilder};
pub use drift::Drift;
pub use schedule::{MissedTickBehavior, PeriodicSchedule, Repeat, Window};

use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    Relative {
        period: Duration,
        window: Option<Window>,
        schedule: PeriodicSchedule,
        on_miss: MissedTickBehavior,
        initial_delay: Duration,
    },
    Absolute {
        period: Duration,
        offset: Option<Duration>,
        window: Option<Window>,
        on_miss: MissedTickBehavior,
    },
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
