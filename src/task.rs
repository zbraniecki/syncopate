use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drift {
    Early(Duration),
    OnTime,
    Late(Duration),
}

impl Drift {
    pub fn as_nanos_signed(&self) -> i128 {
        match self {
            Drift::Early(d) => -(d.as_nanos() as i128),
            Drift::OnTime => 0,
            Drift::Late(d) => d.as_nanos() as i128,
        }
    }
}

impl std::fmt::Display for Drift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Drift::Early(d) => {
                write!(f, "-")?;
                fmt_duration(*d, f)
            }
            Drift::OnTime => write!(f, "0ns"),
            Drift::Late(d) => {
                write!(f, "+")?;
                fmt_duration(*d, f)
            }
        }
    }
}

/// Format a `Duration` using the most readable unit.
fn fmt_duration(d: Duration, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        write!(f, "{}ns", nanos)
    } else if nanos < 1_000_000 {
        write!(f, "{}µs", nanos / 1_000)
    } else if nanos < 1_000_000_000 {
        write!(f, "{}ms", nanos / 1_000_000)
    } else {
        let secs = d.as_secs();
        let ms = (nanos % 1_000_000_000) / 1_000_000;
        if ms == 0 {
            write!(f, "{}s", secs)
        } else {
            write!(f, "{}.{:03}s", secs, ms)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeriodicSchedule {
    /// Period measured from ideal deadline to ideal deadline.
    /// Self-correcting: tick processing time doesn't affect cadence.
    ///
    /// On miss (beyond window): re-aligns to the periodic grid.
    /// Equivalent to tokio's `MissedTickBehavior::Skip`.
    #[default]
    FixedRate,
    /// Period measured from actual fire time.
    /// Tick processing time extends the total cycle.
    ///
    /// On miss (beyond window): next period starts from `now`.
    /// Equivalent to tokio's `MissedTickBehavior::Delay`.
    FixedDelay,
}

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
pub enum TaskType {
    Relative {
        period: Duration,
        window: Window,
        schedule: PeriodicSchedule,
    },
    Anchored {
        period: Duration,
        offset: Option<Duration>,
        window: Window,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Forever,
    Times(u32),
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
    pub on_miss: Option<MissCallback<Ctx>>,
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
    repeat: Repeat,
    priority: u8,
    name: Option<String>,
    on_execute: Option<TaskCallback<Ctx>>,
    on_miss: Option<MissCallback<Ctx>>,
    schedule: PeriodicSchedule,
}

impl<Ctx> TaskBuilder<Ctx> {
    pub fn new() -> Self {
        Self {
            task_type: None,
            repeat: Repeat::Forever,
            priority: 0,
            name: None,
            on_execute: None,
            on_miss: None,
            schedule: PeriodicSchedule::default(),
        }
    }

    // -- Relative builders --

    pub fn every(period: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::Relative {
            period,
            window,
            schedule: PeriodicSchedule::default(),
        })
    }

    pub fn once_after(delay: Duration, window: Window) -> Self {
        let mut b = Self::new().task_type(TaskType::Relative {
            period: delay,
            window,
            schedule: PeriodicSchedule::default(),
        });
        b.repeat = Repeat::Times(1);
        b
    }

    // -- Anchored builders --

    pub fn every_at_boundary(period: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::Anchored {
            period,
            offset: None,
            window,
        })
    }

    pub fn every_with_offset(period: Duration, offset: Duration, window: Window) -> Self {
        Self::new().task_type(TaskType::Anchored {
            period,
            offset: Some(offset),
            window,
        })
    }

    pub fn at_boundary(period: Duration, window: Window) -> Self {
        let mut b = Self::new().task_type(TaskType::Anchored {
            period,
            offset: None,
            window,
        });
        b.repeat = Repeat::Times(1);
        b
    }

    pub fn at_next(period: Duration, offset: Duration, window: Window) -> Self {
        let mut b = Self::new().task_type(TaskType::Anchored {
            period,
            offset: Some(offset),
            window,
        });
        b.repeat = Repeat::Times(1);
        b
    }

    fn task_type(mut self, task_type: TaskType) -> Self {
        self.task_type = Some(task_type);
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

    pub fn on_miss(mut self, callback: MissCallback<Ctx>) -> Self {
        self.on_miss = Some(callback);
        self
    }

    pub fn schedule(mut self, schedule: PeriodicSchedule) -> Self {
        self.schedule = schedule;
        self
    }

    pub fn build(self) -> Result<Task<Ctx>, TaskBuildError> {
        let mut task_type = self.task_type.ok_or(TaskBuildError::NoTaskType)?;

        // Apply the builder's schedule to Relative tasks.
        if let TaskType::Relative { schedule: s, .. } = &mut task_type {
            *s = self.schedule;
        }

        // Validate offset < period for Anchored tasks.
        if let TaskType::Anchored {
            period,
            offset: Some(offset),
            ..
        } = &task_type
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
            repeat: self.repeat,
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
