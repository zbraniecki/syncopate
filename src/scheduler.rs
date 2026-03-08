use crate::system_time::{Clock, MonoInstant, RealClock, WallInstant};
pub use crate::task::Drift;
use crate::task::{MissedTickBehavior, PeriodicSchedule, Repeat, Task, TaskType};
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

struct ScheduledTask<Ctx = ()> {
    task: Task<Ctx>,
    last_fired: Option<MonoInstant>,
    last_wall_deadline: Option<WallInstant>,
    remaining: Option<u32>, // None = forever, Some(n) = n fires left
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
            TaskType::Anchored { period, offset, .. } => {
                let wall_now = self.clock.wall_now();
                let offset_dur = offset.unwrap_or(Duration::ZERO);
                floor_wall_deadline(wall_now, *period, offset_dur)
            }
            _ => None,
        };

        let remaining = match task.repeat {
            Repeat::Forever => None,
            Repeat::Times(n) => Some(n),
        };

        self.tasks.push(ScheduledTask {
            task,
            last_fired: None,
            last_wall_deadline,
            remaining,
        });
        Ok(())
    }

    /// Returns the absolute monotonic deadline for the next tick, adjusted for
    /// `timer_delay`. Use with `tokio::time::sleep_until` for maximum precision.
    pub fn next_tick_deadline(&self) -> Option<MonoInstant> {
        self.soonest_deadline().map(|t| t - self.timer_delay)
    }

    /// Returns how long the caller should sleep before calling `tick`.
    pub fn calculate_next_tick(&self) -> Option<Duration> {
        let now = self.clock.monotonic_now();
        self.soonest_deadline().map(|t| {
            t.saturating_duration_since(now)
                .saturating_sub(self.timer_delay)
        })
    }

    fn soonest_deadline(&self) -> Option<MonoInstant> {
        let mut soonest: Option<MonoInstant> = None;

        for task in &self.tasks {
            // Skip exhausted tasks.
            if task.remaining == Some(0) {
                continue;
            }

            let mono_deadline = match &task.task.task_type {
                TaskType::Relative { period, .. } => {
                    let anchor = task.last_fired.unwrap_or(self.started_at);
                    anchor + *period
                }
                TaskType::Anchored { period, offset, .. } => {
                    let now = self.clock.monotonic_now();
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
            };

            soonest = Some(match soonest {
                None => mono_deadline,
                Some(s) if mono_deadline < s => mono_deadline,
                Some(s) => s,
            });
        }

        soonest
    }

    pub fn tick(&mut self) -> TickResult<'_, Ctx> {
        // Evict tasks exhausted in a previous tick.
        self.tasks.retain(|t| t.remaining != Some(0));

        let now = self.clock.monotonic_now();
        let mut fired = vec![];
        let mut missed = vec![];

        for task in &mut self.tasks {
            match &task.task.task_type {
                TaskType::Relative {
                    period,
                    window,
                    schedule,
                    on_miss,
                } => {
                    let (period, window, schedule, on_miss) =
                        (*period, *window, *schedule, *on_miss);
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
                        if let Some(ref mut r) = task.remaining {
                            *r = r.saturating_sub(1);
                        }
                        fired.push(TaskExecution {
                            task: &task.task,
                            drift,
                        });
                    } else if schedule == PeriodicSchedule::FixedDelay {
                        // FixedDelay always fires — window is informational only.
                        task.last_fired = Some(now);
                        if let Some(ref mut r) = task.remaining {
                            *r = r.saturating_sub(1);
                        }
                        fired.push(TaskExecution {
                            task: &task.task,
                            drift: Drift::Late(now - next_deadline),
                        });
                    } else {
                        // FixedRate: branch on miss behavior.
                        let elapsed_ns = (now - next_deadline).as_nanos();
                        let period_ns = period.as_nanos();
                        let periods_elapsed = elapsed_ns / period_ns;
                        let count = periods_elapsed as u32 + 1;

                        match on_miss {
                            MissedTickBehavior::Execute => {
                                if count > 1 {
                                    let skipped: Vec<Duration> = (0..(count - 1))
                                        .map(|i| {
                                            let offset = period_ns * i as u128;
                                            Duration::from_nanos((elapsed_ns - offset) as u64)
                                        })
                                        .collect();
                                    missed.push(MissedExecution {
                                        task: &task.task,
                                        deadlines_missed: skipped,
                                    });
                                }
                                let latest_deadline =
                                    next_deadline + period * (periods_elapsed as u32);
                                task.last_fired = Some(latest_deadline);
                                if let Some(ref mut r) = task.remaining {
                                    *r = r.saturating_sub(1);
                                }
                                fired.push(TaskExecution {
                                    task: &task.task,
                                    drift: Drift::Late(now - latest_deadline),
                                });
                            }
                            MissedTickBehavior::Burst { max } => {
                                let fire_count = max.map_or(count, |m| count.min(m));
                                let skip_count = count - fire_count;
                                if skip_count > 0 {
                                    let skipped: Vec<Duration> = (0..skip_count)
                                        .map(|i| {
                                            let offset = period_ns * i as u128;
                                            Duration::from_nanos((elapsed_ns - offset) as u64)
                                        })
                                        .collect();
                                    missed.push(MissedExecution {
                                        task: &task.task,
                                        deadlines_missed: skipped,
                                    });
                                }
                                for i in skip_count..count {
                                    let offset = period_ns * i as u128;
                                    let lateness = elapsed_ns - offset;
                                    fired.push(TaskExecution {
                                        task: &task.task,
                                        drift: Drift::Late(Duration::from_nanos(lateness as u64)),
                                    });
                                }
                                task.last_fired = Some(next_deadline + period * (count - 1));
                                if let Some(ref mut r) = task.remaining {
                                    *r = r.saturating_sub(fire_count);
                                }
                            }
                            MissedTickBehavior::Skip => {
                                let deadlines_missed: Vec<Duration> = (0..count)
                                    .map(|i| {
                                        let offset = period_ns * i as u128;
                                        Duration::from_nanos((elapsed_ns - offset) as u64)
                                    })
                                    .collect();
                                task.last_fired =
                                    Some(next_deadline + period * (periods_elapsed as u32));
                                missed.push(MissedExecution {
                                    task: &task.task,
                                    deadlines_missed,
                                });
                            }
                        }
                    }
                }
                TaskType::Anchored {
                    period,
                    offset,
                    window,
                    on_miss,
                } => {
                    let (period, offset_dur, window, on_miss) =
                        (*period, offset.unwrap_or(Duration::ZERO), *window, *on_miss);
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
                        continue;
                    }

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
                        if let Some(ref mut r) = task.remaining {
                            *r = r.saturating_sub(1);
                        }
                        fired.push(TaskExecution {
                            task: &task.task,
                            drift,
                        });
                    } else {
                        let elapsed_ns = wall_now.as_nanos() - deadline.as_nanos();
                        let period_ns = period.as_nanos() as u64;
                        let periods_elapsed = elapsed_ns / period_ns;
                        let count = periods_elapsed as u32 + 1;

                        match on_miss {
                            MissedTickBehavior::Execute => {
                                if count > 1 {
                                    let skipped: Vec<Duration> = (0..(count - 1))
                                        .map(|i| {
                                            let offset = period_ns * i as u64;
                                            Duration::from_nanos(elapsed_ns - offset)
                                        })
                                        .collect();
                                    missed.push(MissedExecution {
                                        task: &task.task,
                                        deadlines_missed: skipped,
                                    });
                                }
                                let last_missed =
                                    WallInstant(deadline.as_nanos() + period_ns * periods_elapsed);
                                task.last_wall_deadline = Some(last_missed);
                                task.last_fired = Some(now);
                                if let Some(ref mut r) = task.remaining {
                                    *r = r.saturating_sub(1);
                                }
                                let lateness = elapsed_ns - period_ns * periods_elapsed;
                                fired.push(TaskExecution {
                                    task: &task.task,
                                    drift: Drift::Late(Duration::from_nanos(lateness)),
                                });
                            }
                            MissedTickBehavior::Burst { max } => {
                                let fire_count = max.map_or(count, |m| count.min(m));
                                let skip_count = count - fire_count;
                                if skip_count > 0 {
                                    let skipped: Vec<Duration> = (0..skip_count)
                                        .map(|i| {
                                            let offset = period_ns * i as u64;
                                            Duration::from_nanos(elapsed_ns - offset)
                                        })
                                        .collect();
                                    missed.push(MissedExecution {
                                        task: &task.task,
                                        deadlines_missed: skipped,
                                    });
                                }
                                for i in skip_count..count {
                                    let offset = period_ns * i as u64;
                                    let lateness = elapsed_ns - offset;
                                    fired.push(TaskExecution {
                                        task: &task.task,
                                        drift: Drift::Late(Duration::from_nanos(lateness)),
                                    });
                                }
                                let last_missed =
                                    WallInstant(deadline.as_nanos() + period_ns * periods_elapsed);
                                task.last_wall_deadline = Some(last_missed);
                                task.last_fired = Some(now);
                                if let Some(ref mut r) = task.remaining {
                                    *r = r.saturating_sub(fire_count);
                                }
                            }
                            MissedTickBehavior::Skip => {
                                let deadlines_missed: Vec<Duration> = (0..count)
                                    .map(|i| {
                                        let offset = period_ns * i as u64;
                                        Duration::from_nanos(elapsed_ns - offset)
                                    })
                                    .collect();
                                let last_missed =
                                    WallInstant(deadline.as_nanos() + period_ns * periods_elapsed);
                                task.last_wall_deadline = Some(last_missed);
                                task.last_fired = Some(now);
                                missed.push(MissedExecution {
                                    task: &task.task,
                                    deadlines_missed,
                                });
                            }
                        }
                    }
                }
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
