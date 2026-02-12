use std::marker::PhantomData;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Timing mode for periodic tasks.
///
/// Periodic tasks repeat indefinitely at regular intervals. There are two timing modes:
/// - **Relative**: Duration-based timing where drift accumulates
/// - **Absolute**: Wall-clock aligned timing with no drift
#[derive(Debug, Clone, PartialEq)]
pub enum PeriodicTiming {
    /// Duration-based timing where each execution schedules the next `period` later.
    ///
    /// Drift accumulates naturally over time since execution overhead and scheduling
    /// delays push each subsequent execution slightly later.
    ///
    /// # Example
    /// ```
    /// use std::time::Duration;
    /// use syncopate::task::TaskBuilder;
    ///
    /// let task = TaskBuilder::<()>::every(Duration::from_secs(1))
    ///     .name("sensor_poll")
    ///     .build()
    ///     .unwrap();
    /// ```
    ///
    /// A 1-second task might fire at: t=1.0s, t=2.001s, t=3.003s, ...
    Relative { period: Duration },

    /// Wall-clock aligned timing where tasks fire at exact period boundaries.
    ///
    /// No drift accumulation - tasks always fire at multiples of `period` from the
    /// scheduler's epoch, optionally shifted by `offset` within each period.
    ///
    /// **Sleep handling**: Automatically resynchronizes after system sleep or
    /// clock adjustments to maintain alignment with wall-clock boundaries.
    ///
    /// # Examples
    ///
    /// **At period boundaries** (offset = None):
    /// ```
    /// use std::time::Duration;
    /// use syncopate::task::TaskBuilder;
    ///
    /// // Fire at exact 1-second boundaries: 0s, 1s, 2s, 3s, ...
    /// let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(1))
    ///     .name("heartbeat")
    ///     .build()
    ///     .unwrap();
    /// ```
    ///
    /// **With offset**:
    /// ```
    /// use std::time::Duration;
    /// use syncopate::task::{TaskBuilder, Time};
    ///
    /// // Fire at :05 of each minute
    /// let task = TaskBuilder::<()>::every_with_offset(
    ///     Duration::from_secs(60),
    ///     Duration::from_secs(5)
    /// )
    /// .name("minute_boundary")
    /// .build()
    /// .unwrap();
    ///
    /// // Fire every day at 16:05 using Time helper
    /// let task = TaskBuilder::<()>::every_with_offset(
    ///     Duration::from_secs(24 * 3600),
    ///     Time::hm(16, 5)
    /// )
    /// .name("daily_report")
    /// .build()
    /// .unwrap();
    /// ```
    Absolute {
        period: Duration,
        offset: Option<Duration>,
    },
}

/// Timing mode for one-time tasks.
///
/// One-time tasks fire once and are then removed from the scheduler. There are two timing modes:
/// - **Relative**: Fire after a delay from when the task is added
/// - **Absolute**: Fire at a specific wall-clock time
#[derive(Debug, Clone, PartialEq)]
pub enum OneTimeTiming {
    /// Fire once after `delay` from when task is added to the scheduler.
    ///
    /// The delay countdown starts when `add_task()` is called, not when the
    /// task is created. This means you can create a task, wait, then add it
    /// later - the delay will be calculated from the add time.
    ///
    /// # Example
    /// ```
    /// use std::time::Duration;
    /// use syncopate::task::TaskBuilder;
    /// use syncopate::scheduler::Scheduler;
    ///
    /// let mut scheduler = Scheduler::new();
    ///
    /// // Create task that fires 5 seconds after being added
    /// let task = TaskBuilder::<()>::once_after(Duration::from_secs(5))
    ///     .name("delayed_action")
    ///     .build()
    ///     .unwrap();
    ///
    /// // Task is created here, but 5-second delay hasn't started yet
    /// // std::thread::sleep(Duration::from_secs(60));
    ///
    /// // Delay starts NOW when we add the task
    /// scheduler.add_task(task).unwrap();
    /// ```
    Relative { delay: Duration },

    /// Fire once at a specific wall-clock time.
    ///
    /// The task will fire when `SystemTime::now()` reaches or exceeds the deadline.
    /// If the deadline is in the past when the task is added, an error is returned.
    ///
    /// # Example
    /// ```
    /// use syncopate::task::{TaskBuilder, Time};
    /// use syncopate::scheduler::Scheduler;
    ///
    /// let mut scheduler = Scheduler::new();
    ///
    /// // Fire at 16:05 today (or tomorrow if already past)
    /// if let Some(deadline) = Time::today_at(16, 5, 0) {
    ///     let task = TaskBuilder::<()>::once_at(deadline)
    ///         .name("reminder")
    ///         .build()
    ///         .unwrap();
    ///
    ///     scheduler.add_task(task).ok();
    /// }
    /// ```
    Absolute { deadline: SystemTime },
}

/// Task timing specification.
///
/// Defines whether a task is periodic (repeating) or one-time, and how it's scheduled.
///
/// # Task Categories
///
/// ## Periodic Tasks
/// Tasks that repeat indefinitely at regular intervals:
/// - [`Periodic(Relative)`](PeriodicTiming::Relative): Duration-based, drift accumulates
/// - [`Periodic(Absolute)`](PeriodicTiming::Absolute): Wall-clock aligned, no drift
///
/// ## One-Time Tasks
/// Tasks that fire once and are then removed:
/// - [`OneTime(Relative)`](OneTimeTiming::Relative): After delay from add time
/// - [`OneTime(Absolute)`](OneTimeTiming::Absolute): At specific wall-clock time
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use syncopate::task::{TaskBuilder, Time};
///
/// // Periodic/Relative - "every second" (drift accumulates)
/// let task1 = TaskBuilder::<()>::every(Duration::from_secs(1))
///     .build()
///     .unwrap();
///
/// // Periodic/Absolute - "every minute at :00" (no drift)
/// let task2 = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
///     .build()
///     .unwrap();
///
/// // Periodic/Absolute with offset - "every hour at :05"
/// let task3 = TaskBuilder::<()>::every_with_offset(
///     Duration::from_secs(3600),
///     Duration::from_secs(5 * 60)
/// )
/// .build()
/// .unwrap();
///
/// // OneTime/Relative - "in 5 seconds"
/// let task4 = TaskBuilder::<()>::once_after(Duration::from_secs(5))
///     .build()
///     .unwrap();
///
/// // OneTime/Absolute - "at 16:05 today"
/// if let Some(deadline) = Time::today_at(16, 5, 0) {
///     let task5 = TaskBuilder::<()>::once_at(deadline)
///         .build()
///         .unwrap();
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    /// Repeating task that fires periodically.
    ///
    /// See [`PeriodicTiming`] for timing modes.
    Periodic(PeriodicTiming),

    /// Task that fires once and is then removed.
    ///
    /// See [`OneTimeTiming`] for timing modes.
    OneTime(OneTimeTiming),
}

impl TaskType {
    /// Get the period for periodic tasks, or None for one-time tasks
    pub fn period(&self) -> Option<Duration> {
        match self {
            TaskType::Periodic(timing) => match timing {
                PeriodicTiming::Relative { period } => Some(*period),
                PeriodicTiming::Absolute { period, .. } => Some(*period),
            },
            TaskType::OneTime(_) => None,
        }
    }
}

pub type TaskCallback<Ctx> = fn(&Ctx);
pub type MissCallback<Ctx> = fn(&Ctx);

pub struct Task<Ctx = ()> {
    pub task_type: TaskType,
    pub priority: u8,
    pub name: Option<String>,
    pub on_execute: Option<TaskCallback<Ctx>>,
    pub on_miss: Option<MissCallback<Ctx>>,
}

#[derive(Debug)]
pub enum TaskBuildError {
    NoTaskType,
    OffsetExceedsPeriod { period: Duration, offset: Duration },
}

impl std::fmt::Display for TaskBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskBuildError::NoTaskType => write!(f, "Task type is required"),
            TaskBuildError::OffsetExceedsPeriod { period, offset } => {
                write!(f, "Offset {:?} exceeds period {:?}", offset, period)
            }
        }
    }
}

impl std::error::Error for TaskBuildError {}

pub struct TaskBuilder<Ctx = ()> {
    task_type: Option<TaskType>,
    priority: u8,
    name: Option<String>,
    on_execute: Option<TaskCallback<Ctx>>,
    on_miss: Option<MissCallback<Ctx>>,
    _phantom: PhantomData<Ctx>,
}

impl<Ctx> TaskBuilder<Ctx> {
    pub fn new() -> Self {
        Self {
            task_type: None,
            priority: 0,
            name: None,
            on_execute: None,
            on_miss: None,
            _phantom: PhantomData,
        }
    }

    /// Create a task that fires `period` after last execution (relative timing, drift accumulates)
    pub fn every(period: Duration) -> Self {
        Self::new().task_type(TaskType::Periodic(PeriodicTiming::Relative { period }))
    }

    /// Create a task that fires at period boundaries (absolute timing, no drift)
    pub fn every_at_boundary(period: Duration) -> Self {
        Self::new().task_type(TaskType::Periodic(PeriodicTiming::Absolute {
            period,
            offset: None,
        }))
    }

    /// Create a task that fires at offset within each period
    /// Example: period=1h, offset=5min → fires at :05 of each hour
    pub fn every_with_offset(period: Duration, offset: Duration) -> Self {
        Self::new().task_type(TaskType::Periodic(PeriodicTiming::Absolute {
            period,
            offset: Some(offset),
        }))
    }

    /// Create a one-time task that fires after delay from when it's added to scheduler
    pub fn once_after(delay: Duration) -> Self {
        Self::new().task_type(TaskType::OneTime(OneTimeTiming::Relative { delay }))
    }

    /// Create a one-time task that fires at specific wall-clock time
    pub fn once_at(deadline: SystemTime) -> Self {
        Self::new().task_type(TaskType::OneTime(OneTimeTiming::Absolute { deadline }))
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

    pub fn build(self) -> Result<Task<Ctx>, TaskBuildError> {
        let task_type = self.task_type.ok_or(TaskBuildError::NoTaskType)?;

        // Validate offset < period for Periodic/Absolute with offset
        if let TaskType::Periodic(PeriodicTiming::Absolute {
            period,
            offset: Some(offset),
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

/// Helper for creating Duration and SystemTime values for scheduling
pub struct Time;

impl Time {
    /// Create a Duration from hours, minutes, seconds
    /// Example: Time::hms(16, 5, 0) for 16:05:00
    pub fn hms(hours: u32, minutes: u32, seconds: u32) -> Duration {
        Duration::from_secs(hours as u64 * 3600 + minutes as u64 * 60 + seconds as u64)
    }

    /// Create a Duration from hours and minutes
    /// Example: Time::hm(16, 5) for 16:05:00
    pub fn hm(hours: u32, minutes: u32) -> Duration {
        Self::hms(hours, minutes, 0)
    }

    /// Create a SystemTime for today at a specific time
    /// Returns the next occurrence (today if not passed, tomorrow if passed)
    ///
    /// Example: Time::today_at(16, 5, 0) for today at 16:05:00
    pub fn today_at(hours: u32, minutes: u32, seconds: u32) -> Option<SystemTime> {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(UNIX_EPOCH).ok()?;

        // Calculate seconds since midnight UTC today
        let secs_today = since_epoch.as_secs() % (24 * 3600);
        let target_secs = hours as u64 * 3600 + minutes as u64 * 60 + seconds as u64;

        if target_secs > secs_today {
            // Target is later today
            let wait = Duration::from_secs(target_secs - secs_today);
            Some(now + wait)
        } else {
            // Target has passed, return tomorrow
            let wait = Duration::from_secs(24 * 3600 - secs_today + target_secs);
            Some(now + wait)
        }
    }

    /// Create a SystemTime for a specific time tomorrow
    /// Example: Time::tomorrow_at(16, 5, 0) for tomorrow at 16:05:00
    pub fn tomorrow_at(hours: u32, minutes: u32, seconds: u32) -> SystemTime {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(UNIX_EPOCH).unwrap();

        let secs_today = since_epoch.as_secs() % (24 * 3600);
        let target_secs = hours as u64 * 3600 + minutes as u64 * 60 + seconds as u64;
        let wait = Duration::from_secs(24 * 3600 - secs_today + target_secs);

        now + wait
    }
}
