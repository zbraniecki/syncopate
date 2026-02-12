use crate::task::{OneTimeTiming, PeriodicTiming, Task, TaskType};
use std::marker::PhantomData;
use std::time::{Duration, SystemTime};

/// Scheduler operating mode
#[derive(Debug, Clone)]
pub enum SchedulerMode {
    /// Production mode - uses real system time via SystemTime::now()
    Production,
    /// Test mode - uses virtual time that must be manually advanced
    Test { initial_time: SystemTime },
}

#[derive(Debug)]
pub enum AddTaskError {
    DeadlineInPast,
    ClockWentBackward,
}

impl std::fmt::Display for AddTaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddTaskError::DeadlineInPast => write!(f, "Deadline is in the past"),
            AddTaskError::ClockWentBackward => write!(f, "Clock went backward"),
        }
    }
}

impl std::error::Error for AddTaskError {}

struct ScheduledTask<Ctx = ()> {
    task: Task<Ctx>,
    next_fire: Duration, // absolute time from scheduler start when this task next fires
    one_time: bool,      // true for one-time tasks that should be removed after execution
    fired: bool,         // true if this one-time task has already fired
}

impl<Ctx> ScheduledTask<Ctx> {
    fn period(&self) -> Option<Duration> {
        self.task.task_type.period()
    }

    /// Calculate time until next execution given current elapsed time.
    fn time_until_next(&self, elapsed: Duration) -> Duration {
        self.next_fire.saturating_sub(elapsed)
    }

    /// Check if task should execute at the given elapsed time.
    fn is_ready(&self, elapsed: Duration) -> bool {
        elapsed >= self.next_fire
    }

    /// Advance to the next firing time.
    fn advance(&mut self) {
        if let Some(period) = self.period() {
            self.next_fire += period;
        }
    }
}

/// Task scheduler with support for relative and absolute timing.
///
/// # Modes
///
/// The scheduler operates in one of two modes, selected at construction:
///
/// - **Production mode** (`Scheduler::new()`): Uses real system time via
///   `SystemTime::now()`. Automatically detects system sleep and clock adjustments.
///
/// - **Test mode** (`Scheduler::with_test_time()`): Uses virtual time that must
///   be manually advanced via `advance_time()`. Provides deterministic behavior
///   for tests.
///
/// Once a scheduler is created, all methods (`add_task`, `tick`, `advance_time`)
/// work the same way regardless of mode. The mode only affects how time is obtained
/// internally.
///
/// # Time Discontinuity Handling
///
/// The scheduler automatically detects time discontinuities (system sleep, clock
/// adjustments) and resynchronizes absolute timing tasks to maintain wall-clock
/// alignment. When wall-clock time advances more than 3x the tick duration,
/// absolute tasks are recalculated:
///
/// - **Periodic absolute tasks**: Resynchronized to next wall-clock boundary
/// - **One-time absolute tasks**: Recalculated from current time to deadline
/// - **Relative tasks**: Unaffected (use virtual time, not wall-clock)
///
/// # Example: Production Mode
///
/// ```
/// use std::time::Duration;
/// use syncopate::task::TaskBuilder;
/// use syncopate::scheduler::Scheduler;
///
/// let mut scheduler = Scheduler::new();
///
/// // Task fires at :00 of each minute
/// let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
///     .build()
///     .unwrap();
///
/// scheduler.add_task(task).unwrap();
///
/// // Machine sleeps from 10:00:30 to 11:00:30
/// // After wake, task automatically resyncs to fire at 11:01:00
/// // (not at 11:00:30 as it would without resync)
/// ```
///
/// # Example: Test Mode
///
/// ```
/// use std::time::{Duration, SystemTime, UNIX_EPOCH};
/// use syncopate::task::TaskBuilder;
/// use syncopate::scheduler::Scheduler;
///
/// let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
/// let mut scheduler = Scheduler::with_test_time(epoch, epoch);
///
/// let task = TaskBuilder::<()>::every(Duration::from_secs(60))
///     .build()
///     .unwrap();
///
/// scheduler.add_task(task).unwrap();
///
/// // Manually advance time
/// scheduler.advance_time(epoch + Duration::from_secs(60));
/// let fired = scheduler.tick(Duration::from_secs(60));
/// ```
pub struct Scheduler<Ctx = ()> {
    mode: SchedulerMode, // operating mode (production or test)
    tasks: Vec<ScheduledTask<Ctx>>,
    elapsed: Duration,          // virtual time since scheduler start
    epoch_start: SystemTime,    // when scheduler started (elapsed=0)
    current_time: SystemTime,   // current wall-clock time
    last_tick_time: SystemTime, // wall-clock time at last tick (for discontinuity detection)
    _phantom: PhantomData<Ctx>,
}

impl<Ctx> Scheduler<Ctx> {
    /// Create scheduler in production mode (uses real system time)
    pub fn new() -> Self {
        let now = SystemTime::now();
        Self {
            mode: SchedulerMode::Production,
            tasks: Vec::new(),
            elapsed: Duration::ZERO,
            epoch_start: now,
            current_time: now,
            last_tick_time: now,
            _phantom: PhantomData,
        }
    }

    /// Create scheduler in test mode (uses virtual time)
    ///
    /// # Arguments
    /// * `start_time` - SystemTime to use as epoch (when elapsed=0)
    /// * `current_time` - Current SystemTime position for anchor calculations
    ///
    /// # Example
    /// ```
    /// use std::time::{SystemTime, UNIX_EPOCH, Duration};
    /// use syncopate::scheduler::Scheduler;
    ///
    /// // Simulate starting at midnight, currently at 16:05
    /// let midnight = UNIX_EPOCH + Duration::from_secs(1700000000);
    /// let now = midnight + Duration::from_secs(16 * 3600 + 5 * 60);
    /// let scheduler: Scheduler = Scheduler::with_test_time(midnight, now);
    /// ```
    pub fn with_test_time(start_time: SystemTime, current_time: SystemTime) -> Self {
        Self {
            mode: SchedulerMode::Test {
                initial_time: current_time,
            },
            tasks: Vec::new(),
            elapsed: Duration::ZERO,
            epoch_start: start_time,
            current_time,
            last_tick_time: current_time,
            _phantom: PhantomData,
        }
    }

    /// Advance the scheduler's clock to a new time
    ///
    /// In test mode, this sets the virtual time.
    /// In production mode, this does nothing (time is always SystemTime::now()).
    ///
    /// # Example
    /// ```
    /// use std::time::{Duration, SystemTime, UNIX_EPOCH};
    /// use syncopate::scheduler::Scheduler;
    ///
    /// let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    /// let mut scheduler = Scheduler::with_test_time(epoch, epoch);
    ///
    /// // Advance time by 1 minute
    /// scheduler.advance_time(epoch + Duration::from_secs(60));
    /// ```
    pub fn advance_time(&mut self, time: SystemTime) {
        match &self.mode {
            SchedulerMode::Test { .. } => {
                self.current_time = time;
            }
            SchedulerMode::Production => {
                // In production mode, time is always real - ignore this call
            }
        }
    }

    /// Add a task to the scheduler
    ///
    /// In production mode, updates current time from SystemTime::now().
    /// In test mode, uses the virtual time set via advance_time().
    pub fn add_task(&mut self, task: Task<Ctx>) -> Result<(), AddTaskError> {
        // Update current time based on mode
        match &self.mode {
            SchedulerMode::Production => {
                self.current_time = SystemTime::now();
            }
            SchedulerMode::Test { .. } => {
                // Use current_time field (set via advance_time)
            }
        }

        let next_fire = self.calculate_next_fire(&task.task_type)?;
        let one_time = matches!(task.task_type, TaskType::OneTime(_));

        self.tasks.push(ScheduledTask {
            task,
            next_fire,
            one_time,
            fired: false,
        });

        Ok(())
    }

    pub fn calculate_next_tick(&self) -> Option<Duration> {
        self.tasks
            .iter()
            .filter(|t| !t.fired) // Skip tasks that have already fired
            .map(|t| t.time_until_next(self.elapsed))
            .min()
    }

    /// Advance time by the given duration.
    /// Returns tasks that are ready to execute.
    ///
    /// In production mode, updates time from SystemTime::now() and detects real discontinuities.
    /// In test mode, uses virtual time set via advance_time().
    pub fn tick(&mut self, duration: Duration) -> Vec<&Task<Ctx>> {
        // Clean up tasks that fired in previous ticks
        self.tasks.retain(|t| !t.fired);

        // Update time and detect discontinuities based on mode
        match &self.mode {
            SchedulerMode::Production => {
                let new_time = SystemTime::now();

                // Detect time discontinuity (system sleep, clock adjustment)
                if let Ok(actual_elapsed) = new_time.duration_since(self.last_tick_time) {
                    let threshold = duration.saturating_mul(3);
                    if actual_elapsed > threshold {
                        let _ = self.resync_absolute_tasks();
                    }
                }

                self.last_tick_time = new_time;
                self.current_time = new_time;
            }
            SchedulerMode::Test { .. } => {
                // Use manually set current_time
                if let Ok(actual_elapsed) = self.current_time.duration_since(self.last_tick_time) {
                    let threshold = duration.saturating_mul(3);
                    if actual_elapsed > threshold {
                        let _ = self.resync_absolute_tasks();
                    }
                }

                self.last_tick_time = self.current_time;
            }
        }

        self.elapsed += duration;

        // Find indices of ready tasks
        let ready_indices: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_ready(self.elapsed))
            .map(|(i, _)| i)
            .collect();

        // Advance ready tasks (periodic ones) and mark one-time tasks as fired
        for &idx in &ready_indices {
            if self.tasks[idx].one_time {
                self.tasks[idx].fired = true;
            } else {
                self.tasks[idx].advance();
            }
        }

        // Collect and return task references
        ready_indices
            .iter()
            .map(|&idx| &self.tasks[idx].task)
            .collect()
    }

    fn calculate_next_fire(&self, task_type: &TaskType) -> Result<Duration, AddTaskError> {
        match task_type {
            TaskType::Periodic(timing) => match timing {
                PeriodicTiming::Relative { period } => {
                    // Fire one period from now (in scheduler time)
                    Ok(self.elapsed + *period)
                }

                PeriodicTiming::Absolute { period, offset } => {
                    // Fire at wall-clock boundaries with optional offset
                    self.calculate_absolute_fire(*period, *offset)
                }
            },

            TaskType::OneTime(timing) => match timing {
                OneTimeTiming::Relative { delay } => {
                    // Fire after delay from now (in scheduler time)
                    // This is calculated when add_task() is called, not when task is created
                    Ok(self.elapsed + *delay)
                }

                OneTimeTiming::Absolute { deadline } => {
                    // Fire when wall-clock reaches deadline
                    let time_until_deadline = deadline
                        .duration_since(self.current_time)
                        .map_err(|_| AddTaskError::DeadlineInPast)?;

                    Ok(self.elapsed + time_until_deadline)
                }
            },
        }
    }

    fn calculate_absolute_fire(
        &self,
        period: Duration,
        offset: Option<Duration>,
    ) -> Result<Duration, AddTaskError> {
        // Calculate position in period cycle from epoch_start
        let since_epoch = self
            .current_time
            .duration_since(self.epoch_start)
            .map_err(|_| AddTaskError::ClockWentBackward)?;

        let period_nanos = period.as_nanos();
        let offset_nanos = offset.unwrap_or(Duration::ZERO).as_nanos();

        // Where are we in the period cycle?
        let current_phase = since_epoch.as_nanos() % period_nanos;

        // Target phase (offset within period)
        let target_phase = offset_nanos;

        // Calculate nanos until next target phase
        let nanos_until_target = if current_phase < target_phase {
            target_phase - current_phase
        } else {
            period_nanos - current_phase + target_phase
        };

        // Convert to scheduler time
        Ok(self.elapsed + Duration::from_nanos(nanos_until_target as u64))
    }

    /// Resynchronize absolute tasks after time discontinuity (system sleep, clock adjustment)
    fn resync_absolute_tasks(&mut self) -> Result<(), AddTaskError> {
        for i in 0..self.tasks.len() {
            match &self.tasks[i].task.task_type {
                // Recalculate periodic absolute tasks to align with current wall-clock boundaries
                TaskType::Periodic(PeriodicTiming::Absolute { period, offset }) => {
                    let new_fire = self.calculate_absolute_fire(*period, *offset)?;
                    self.tasks[i].next_fire = new_fire;
                }

                // Recalculate one-time absolute tasks (may have passed deadline during sleep)
                TaskType::OneTime(OneTimeTiming::Absolute { deadline }) => {
                    // Calculate time until deadline from current wall-clock
                    let time_until = deadline
                        .duration_since(self.current_time)
                        .map_err(|_| AddTaskError::DeadlineInPast)?;

                    self.tasks[i].next_fire = self.elapsed + time_until;
                }

                // Relative tasks don't need resync - they're based on virtual elapsed time
                _ => {}
            }
        }
        Ok(())
    }
}

impl<Ctx> Default for Scheduler<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}
