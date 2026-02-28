use crate::system_time::{Clock, MonoInstant, RealClock};
use crate::task::{PeriodicTiming, Task, TaskType};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddTaskError {
    #[error("Deadline is in the past")]
    DeadlineInPast,
    #[error("Clock went backward")]
    ClockWentBackward,
}

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

#[derive(Debug)]
pub struct TaskExecution<'a, Ctx = ()> {
    pub task: &'a Task<Ctx>,
    pub drift: Drift,
}

#[derive(Debug)]
pub struct TickResult<'a, Ctx = ()> {
    pub fired: Vec<TaskExecution<'a, Ctx>>,
    pub missed: Vec<TaskExecution<'a, Ctx>>,
}

struct ScheduledTask<Ctx = ()> {
    task: Task<Ctx>,
    last_fired: Option<MonoInstant>,
}

pub struct Scheduler<Ctx = (), C: Clock = RealClock> {
    clock: C,
    started_at: MonoInstant,
    timer_delay: Duration,
    tasks: Vec<ScheduledTask<Ctx>>,
}

/// Constructors for the default `RealClock` configuration.
impl<Ctx> Scheduler<Ctx> {
    pub fn new() -> Self {
        Self::new_with_clock(RealClock::new())
    }
}

impl<Ctx> Default for Scheduler<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

/// Core API — available for any clock.
impl<Ctx, C: Clock> Scheduler<Ctx, C> {
    pub fn new_with_clock(clock: C) -> Self {
        let started_at = clock.monotonic_now();
        Self {
            clock,
            started_at,
            timer_delay: Duration::ZERO,
            tasks: vec![],
        }
    }

    pub fn set_timer_delay(&mut self, timer_delay: Duration) {
        self.timer_delay = timer_delay;
    }

    pub fn add_task(&mut self, task: Task<Ctx>) -> Result<(), AddTaskError> {
        self.tasks.push(ScheduledTask {
            task,
            last_fired: None,
        });
        Ok(())
    }

    /// Returns how long the caller should sleep before calling `tick`.
    ///
    /// For each relative periodic task the next wakeup point is
    /// `(anchor + period) - window.early` — the earliest moment the task is
    /// allowed to fire.  We return the duration from *now* until the soonest
    /// such point across all tasks, minus `timer_delay` to compensate for
    /// known OS wakeup latency.
    ///
    /// Returns `Some(Duration::ZERO)` when a task is already inside or past
    /// its window (tick should be called immediately), and `None` when there
    /// are no schedulable tasks.
    pub fn calculate_next_tick(&self) -> Option<Duration> {
        let now = self.clock.monotonic_now();
        let mut soonest: Option<MonoInstant> = None;

        for task in &self.tasks {
            let (period, window) = match &task.task.task_type {
                TaskType::Periodic(PeriodicTiming::Relative { period, window, .. }) => {
                    (*period, *window)
                }
                _ => continue,
            };

            let anchor = task.last_fired.unwrap_or(self.started_at);
            let deadline = anchor + period;

            soonest = Some(match soonest {
                None => deadline,
                Some(s) if deadline < s => deadline,
                Some(s) => s,
            });
        }

        soonest.map(|t| {
            t.saturating_duration_since(now)
                .saturating_sub(self.timer_delay)
        })
    }

    pub fn tick(&mut self) -> TickResult<'_, Ctx> {
        let now = self.clock.monotonic_now();
        let mut fired = vec![];
        let mut missed = vec![];

        for task in &mut self.tasks {
            let (period, window) = match &task.task.task_type {
                TaskType::Periodic(PeriodicTiming::Relative { period, window, .. }) => {
                    (*period, *window)
                }
                _ => continue,
            };

            let anchor = task.last_fired.unwrap_or(self.started_at);
            let next_deadline = anchor + period;

            let window_start = next_deadline - window.early;
            let window_end = next_deadline + window.late;

            if now < window_start {
                // Before the window — not yet due.
                continue;
            }

            let drift = calculate_drift(now, next_deadline);

            if now <= window_end {
                // Within the window — fire.
                task.last_fired = Some(now);
                fired.push(TaskExecution {
                    task: &task.task,
                    drift,
                });
            } else {
                // Past window_end — missed.
                // Advance last_fired so the next tick doesn't re-report the same miss.
                task.last_fired = Some(now);
                missed.push(TaskExecution {
                    task: &task.task,
                    drift,
                });
            }
        }

        TickResult { fired, missed }
    }
}

/// Compute the drift between an actual wakeup (`now`) and the ideal `deadline`.
fn calculate_drift(now: MonoInstant, deadline: MonoInstant) -> Drift {
    if now > deadline {
        Drift::Late(now - deadline)
    } else if now < deadline {
        Drift::Early(deadline - now)
    } else {
        Drift::OnTime
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
