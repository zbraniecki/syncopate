use crate::task::{
    MissCallback, MissedTickBehavior, PeriodicSchedule, Repeat, Task, TaskCallback, TaskType,
    Window,
};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskBuildError {
    #[error("Offset {offset:?} exceeds period {period:?}")]
    OffsetExceedsPeriod { period: Duration, offset: Duration },
    #[error("Offset cannot be set on relative tasks")]
    OffsetOnRelativeTask,
}

enum TaskKind {
    Relative,
    Absolute,
}

pub struct TaskBuilder<Ctx = ()> {
    kind: TaskKind,
    period: Duration,
    window: Option<Window>,
    offset: Option<Duration>,
    repeat: Repeat,
    priority: u8,
    name: Option<String>,
    on_execute: Option<TaskCallback<Ctx>>,
    on_missed: Option<MissCallback<Ctx>>,
    schedule: PeriodicSchedule,
    on_miss: MissedTickBehavior,
    initial_delay: Duration,
}

impl<Ctx> TaskBuilder<Ctx> {
    pub fn every(period: Duration) -> Self {
        Self {
            kind: TaskKind::Relative,
            period,
            window: None,
            offset: None,
            repeat: Repeat::Forever,
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: Duration::ZERO,
        }
    }

    pub fn every_absolute(period: Duration) -> Self {
        Self {
            kind: TaskKind::Absolute,
            period,
            window: None,
            offset: None,
            repeat: Repeat::Forever,
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: Duration::ZERO,
        }
    }

    pub fn once_after(delay: Duration) -> Self {
        Self {
            kind: TaskKind::Relative,
            period: delay,
            window: None,
            offset: None,
            repeat: Repeat::Times(1),
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: delay,
        }
    }

    pub fn once_at(period: Duration) -> Self {
        Self {
            kind: TaskKind::Absolute,
            period,
            window: None,
            offset: None,
            repeat: Repeat::Times(1),
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: Duration::ZERO,
        }
    }

    pub fn window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }

    pub fn offset(mut self, offset: Duration) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn once(mut self) -> Self {
        self.repeat = Repeat::Times(1);
        self
    }

    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn on_execute(mut self, callback: TaskCallback<Ctx>) -> Self {
        self.on_execute = Some(callback);
        self
    }

    pub fn on_missed(mut self, callback: MissCallback<Ctx>) -> Self {
        self.on_missed = Some(callback);
        self
    }

    pub fn on_miss(mut self, behavior: MissedTickBehavior) -> Self {
        self.on_miss = behavior;
        self
    }

    pub fn schedule(mut self, schedule: PeriodicSchedule) -> Self {
        self.schedule = schedule;
        self
    }

    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    pub fn build(self) -> Result<Task<Ctx>, TaskBuildError> {
        let task_type = match self.kind {
            TaskKind::Relative => {
                if self.offset.is_some() {
                    return Err(TaskBuildError::OffsetOnRelativeTask);
                }
                TaskType::Relative {
                    period: self.period,
                    window: self.window,
                    schedule: self.schedule,
                    on_miss: self.on_miss,
                    initial_delay: self.initial_delay,
                }
            }
            TaskKind::Absolute => {
                if let Some(offset) = self.offset
                    && offset >= self.period {
                        return Err(TaskBuildError::OffsetExceedsPeriod {
                            period: self.period,
                            offset,
                        });
                    }
                TaskType::Absolute {
                    period: self.period,
                    offset: self.offset,
                    window: self.window,
                    on_miss: self.on_miss,
                }
            }
        };

        Ok(Task {
            task_type,
            repeat: self.repeat,
            priority: self.priority,
            name: self.name,
            on_execute: self.on_execute,
            on_missed: self.on_missed,
        })
    }
}
