use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub early: Duration,
    pub late: Duration,
}

impl Window {
    pub const fn new(early: Duration, late: Duration) -> Self {
        Self { early, late }
    }

    pub const ZERO: Self = Self {
        early: Duration::ZERO,
        late: Duration::ZERO,
    };

    pub const fn symmetric(margin: Duration) -> Self {
        Self {
            early: margin,
            late: margin,
        }
    }

    pub const fn is_zero(&self) -> bool {
        self.early.as_nanos() == 0 && self.late.as_nanos() == 0
    }

    pub const fn total(&self) -> Duration {
        Duration::from_nanos(self.early.as_nanos() as u64 + self.late.as_nanos() as u64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeriodicTiming {
    Relative {
        period: Duration,
        window: Window,
        consecutive_window: Option<Window>,
    },

    Absolute {
        period: Duration,
        offset: Option<Duration>,
        window: Window,
        consecutive_window: Option<Window>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OneTimeTiming {
    Relative {
        delay: Duration,
        window: Window,
    },

    Absolute {
        deadline: SystemTime,
        window: Window,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    Periodic(PeriodicTiming),

    OneTime(OneTimeTiming),
}

impl TaskType {}

pub type TaskCallback<Ctx> = fn(&Ctx);
pub type MissCallback<Ctx> = fn(&Ctx);

#[derive(Debug)]
pub struct Task<Ctx = ()> {
    pub task_type: TaskType,
    pub priority: u8,
    pub name: Option<String>,
    pub on_execute: Option<TaskCallback<Ctx>>,
    pub on_miss: Option<MissCallback<Ctx>>,
}

impl<Ctx> Task<Ctx> {
    pub fn next_fire(&self) -> Duration {
        match &self.task_type {
            TaskType::Periodic(periodic_timing) => match periodic_timing {
                PeriodicTiming::Relative { period, .. } => {
                    return *period;
                }
                PeriodicTiming::Absolute { .. } => todo!(),
            },
            TaskType::OneTime(..) => todo!(),
        }
    }
}

#[derive(Debug, Error)]
pub enum TaskBuildError {
    #[error("Task type is required")]
    NoTaskType,
    #[error("Offset {offset:?} exceeds period {period:?}")]
    OffsetExceedsPeriod { period: Duration, offset: Duration },
}

pub struct TaskBuilder<Ctx = ()> {
    task_type: Option<TaskType>,
    priority: u8,
    name: Option<String>,
    on_execute: Option<TaskCallback<Ctx>>,
    on_miss: Option<MissCallback<Ctx>>,
    consecutive_window: Option<Window>,
}

impl<Ctx> TaskBuilder<Ctx> {
    pub fn new() -> Self {
        Self {
            task_type: None,
            priority: 0,
            name: None,
            on_execute: None,
            on_miss: None,
            consecutive_window: None,
        }
    }

    pub fn every(period: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::Periodic(PeriodicTiming::Relative {
            period,
            window,
            consecutive_window: None,
        }))
    }

    pub fn every_at_boundary(period: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::Periodic(PeriodicTiming::Absolute {
            period,
            offset: None,
            window,
            consecutive_window: None,
        }))
    }

    pub fn every_with_offset(period: Duration, offset: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::Periodic(PeriodicTiming::Absolute {
            period,
            offset: Some(offset),
            window,
            consecutive_window: None,
        }))
    }

    pub fn once_after(delay: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::OneTime(OneTimeTiming::Relative { delay, window }))
    }

    pub fn once_at(deadline: SystemTime, window: Window) -> Self {
        Self::new().task_type(TaskType::OneTime(OneTimeTiming::Absolute {
            deadline,
            window,
        }))
    }

    pub fn task_type(mut self, task_type: TaskType) -> Self {
        self.task_type = Some(task_type);
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

    pub fn on_miss(mut self, callback: MissCallback<Ctx>) -> Self {
        self.on_miss = Some(callback);
        self
    }

    pub fn with_consecutive_window(mut self, window: Window) -> Self {
        self.consecutive_window = Some(window);
        self
    }

    pub fn build(self) -> Result<Task<Ctx>, TaskBuildError> {
        let mut task_type = self.task_type.ok_or(TaskBuildError::NoTaskType)?;

        if let Some(consecutive_window) = self.consecutive_window {
            match &mut task_type {
                TaskType::Periodic(PeriodicTiming::Relative {
                    consecutive_window: cw,
                    ..
                }) => {
                    *cw = Some(consecutive_window);
                }
                TaskType::Periodic(PeriodicTiming::Absolute {
                    consecutive_window: cw,
                    ..
                }) => {
                    *cw = Some(consecutive_window);
                }
                TaskType::OneTime(_) => {}
            }
        }

        if let TaskType::Periodic(PeriodicTiming::Absolute {
            period,
            offset: Some(offset),
            ..
        }) = &task_type
        {
            if offset >= period {
                return Err(TaskBuildError::OffsetExceedsPeriod {
                    period: *period,
                    offset: *offset,
                });
            }
        }

        Ok(Task {
            task_type,
            priority: self.priority,
            name: self.name,
            on_execute: self.on_execute,
            on_miss: self.on_miss,
        })
    }
}

impl<Ctx> Default for TaskBuilder<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Time;

impl Time {
    pub fn hms(hours: u32, minutes: u32, seconds: u32) -> Duration {
        Duration::from_secs(hours as u64 * 3600 + minutes as u64 * 60 + seconds as u64)
    }

    pub fn hm(hours: u32, minutes: u32) -> Duration {
        Self::hms(hours, minutes, 0)
    }

    pub fn today_at(hours: u32, minutes: u32, seconds: u32) -> Option<SystemTime> {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(UNIX_EPOCH).ok()?;

        let secs_today = since_epoch.as_secs() % (24 * 3600);
        let target_secs = hours as u64 * 3600 + minutes as u64 * 60 + seconds as u64;

        if target_secs > secs_today {
            let wait = Duration::from_secs(target_secs - secs_today);
            Some(now + wait)
        } else {
            let wait = Duration::from_secs(24 * 3600 - secs_today + target_secs);
            Some(now + wait)
        }
    }

    pub fn tomorrow_at(hours: u32, minutes: u32, seconds: u32) -> SystemTime {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(UNIX_EPOCH).unwrap();

        let secs_today = since_epoch.as_secs() % (24 * 3600);
        let target_secs = hours as u64 * 3600 + minutes as u64 * 60 + seconds as u64;
        let wait = Duration::from_secs(24 * 3600 - secs_today + target_secs);

        now + wait
    }
}
