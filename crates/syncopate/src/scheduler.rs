use crate::system_time::{Clock, MonoInstant, RealClock, WallInstant};
use crate::task::{PeriodicSchedule, PeriodicTiming, Task, TaskType};
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
    last_wall_deadline: Option<WallInstant>,
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
        let last_wall_deadline = match &task.task_type {
            TaskType::Periodic(PeriodicTiming::Absolute { period, offset, .. }) => {
                let wall_now = self.clock.wall_now();
                let offset_dur = offset.unwrap_or(Duration::ZERO);
                floor_wall_deadline(wall_now, *period, offset_dur)
            }
            _ => None,
        };

        self.tasks.push(ScheduledTask {
            task,
            last_fired: None,
            last_wall_deadline,
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
            let mono_deadline = match &task.task.task_type {
                TaskType::Periodic(PeriodicTiming::Relative { period, .. }) => {
                    let anchor = task.last_fired.unwrap_or(self.started_at);
                    anchor + *period
                }
                TaskType::Periodic(PeriodicTiming::Absolute { period, offset, .. }) => {
                    let wall_now = self.clock.wall_now();
                    let offset_dur = offset.unwrap_or(Duration::ZERO);
                    let deadline = next_absolute_deadline(
                        wall_now,
                        *period,
                        offset_dur,
                        task.last_wall_deadline,
                    );

                    let wait = if deadline > wall_now {
                        Duration::from_nanos(deadline.as_nanos() - wall_now.as_nanos())
                    } else {
                        Duration::ZERO
                    };

                    now + wait
                }
                _ => continue,
            };

            soonest = Some(match soonest {
                None => mono_deadline,
                Some(s) if mono_deadline < s => mono_deadline,
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
            match &task.task.task_type {
                TaskType::Periodic(PeriodicTiming::Relative {
                    period,
                    window,
                    schedule,
                    ..
                }) => {
                    let (period, window, schedule) = (*period, *window, *schedule);
                    let anchor = task.last_fired.unwrap_or(self.started_at);
                    let next_deadline = anchor + period;

                    let window_start = next_deadline - window.early;
                    let window_end = next_deadline + window.late;

                    if now < window_start {
                        continue;
                    }

                    let drift = calculate_drift(now, next_deadline);

                    if now <= window_end {
                        task.last_fired = Some(match schedule {
                            PeriodicSchedule::FixedRate => next_deadline,
                            PeriodicSchedule::FixedDelay => now,
                        });
                        fired.push(TaskExecution {
                            task: &task.task,
                            drift,
                        });
                    } else {
                        task.last_fired = Some(now);
                        missed.push(TaskExecution {
                            task: &task.task,
                            drift,
                        });
                    }
                }
                TaskType::Periodic(PeriodicTiming::Absolute {
                    period,
                    offset,
                    window,
                    ..
                }) => {
                    let (period, offset_dur, window) =
                        (*period, offset.unwrap_or(Duration::ZERO), *window);
                    let wall_now = self.clock.wall_now();

                    let deadline = next_absolute_deadline(
                        wall_now,
                        period,
                        offset_dur,
                        task.last_wall_deadline,
                    );

                    let window_start = deadline - window.early;
                    let window_end = deadline + window.late;

                    if wall_now < window_start {
                        continue; // Not yet due.
                    }

                    // Drift is wall-clock based for absolute tasks.
                    let drift = if wall_now.as_nanos() > deadline.as_nanos() {
                        Drift::Late(Duration::from_nanos(
                            wall_now.as_nanos() - deadline.as_nanos(),
                        ))
                    } else if wall_now.as_nanos() < deadline.as_nanos() {
                        Drift::Early(Duration::from_nanos(
                            deadline.as_nanos() - wall_now.as_nanos(),
                        ))
                    } else {
                        Drift::OnTime
                    };

                    if wall_now <= window_end {
                        task.last_wall_deadline = Some(deadline);
                        task.last_fired = Some(now);
                        fired.push(TaskExecution {
                            task: &task.task,
                            drift,
                        });
                    } else {
                        task.last_wall_deadline = Some(deadline);
                        task.last_fired = Some(now);
                        missed.push(TaskExecution {
                            task: &task.task,
                            drift,
                        });
                    }
                }
                _ => continue,
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

/// Given the current wall time and the task's last serviced deadline, return
/// the next deadline that needs evaluation.
///
/// After an early fire, `last_wall_deadline` may be *ahead* of the floor
/// deadline (we serviced a future boundary). The `>=` comparison handles this
/// correctly — any floor deadline at or below the last serviced one is already
/// covered.
fn next_absolute_deadline(
    wall_now: WallInstant,
    period: Duration,
    offset_dur: Duration,
    last_wall_deadline: Option<WallInstant>,
) -> WallInstant {
    match floor_wall_deadline(wall_now, period, offset_dur) {
        Some(current) if last_wall_deadline.is_some_and(|last| last >= current) => {
            last_wall_deadline.unwrap() + period
        }
        Some(current) => current,
        None => match last_wall_deadline {
            Some(last) if last >= WallInstant(offset_dur.as_nanos() as u64) => last + period,
            _ => WallInstant(offset_dur.as_nanos() as u64),
        },
    }
}

/// Compute the most recent wall-clock deadline at or before `wall_now`.
///
/// Deadlines form the series: offset, offset + period, offset + 2·period, …
/// Returns `None` if `wall_now` is before the first deadline (`offset`).
fn floor_wall_deadline(
    wall_now: WallInstant,
    period: Duration,
    offset: Duration,
) -> Option<WallInstant> {
    let now_nanos = wall_now.as_nanos();
    let offset_nanos = offset.as_nanos() as u64;

    if now_nanos < offset_nanos {
        return None;
    }

    let period_nanos = period.as_nanos() as u64;
    let aligned = ((now_nanos - offset_nanos) / period_nanos) * period_nanos + offset_nanos;
    Some(WallInstant(aligned))
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
