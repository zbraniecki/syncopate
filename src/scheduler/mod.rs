mod deadline;
mod tick;
mod types;

pub use types::{MissedExecution, TaskExecution, TickResult};

use crate::clock::{Clock, MonoInstant, RealClock, WallInstant};
use crate::task::{Drift, Repeat, Task, TaskType, Window};
use deadline::{floor_wall_deadline, next_absolute_deadline};
use std::cmp::Ordering;
use std::time::Duration;
use tick::{tick_absolute, tick_relative};
use types::{ScheduledTask, TaskState};

pub struct Scheduler<Ctx = (), C: Clock = RealClock> {
    clock: C,
    timer_delay: Duration,
    min_tick_interval: Option<Duration>,
    tasks: Vec<ScheduledTask<Ctx>>,
    running: bool,
}

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

impl<Ctx, C: Clock> Scheduler<Ctx, C> {
    pub fn new_with_clock(clock: C) -> Self {
        Self {
            clock,
            timer_delay: Duration::ZERO,
            min_tick_interval: None,
            tasks: vec![],
            running: false,
        }
    }

    pub fn set_timer_delay(&mut self, timer_delay: Duration) {
        self.timer_delay = timer_delay;
    }

    pub fn set_min_tick_interval(&mut self, interval: Option<Duration>) {
        self.min_tick_interval = interval;
    }

    pub fn add_task(&mut self, task: Task<Ctx>) -> Option<Drift> {
        let now = self.clock.monotonic_now();
        let remaining = match task.repeat {
            Repeat::Forever => None,
            Repeat::Times(n) => Some(n),
        };

        let (state, immediate_drift) = match &task.task_type {
            TaskType::Relative(data) => {
                let fires_now = self.running && data.initial_delay == Duration::ZERO;
                let state = TaskState {
                    added_at: now,
                    last_fired: fires_now.then_some(now),
                    last_wall_deadline: None,
                    remaining: if fires_now {
                        remaining.map(|r| r.saturating_sub(1))
                    } else {
                        remaining
                    },
                };
                (state, fires_now.then_some(Drift::OnTime))
            }

            TaskType::Absolute(data) => {
                let wall_now = self.clock.wall_now();
                let offset_dur = data.offset.unwrap_or(Duration::ZERO);

                let floor = floor_wall_deadline(wall_now, data.period, offset_dur);
                let anchored_deadline = match floor {
                    Some(f) if f == wall_now => None,
                    other => other,
                };

                let immediate_drift = if self.running {
                    let deadline = next_absolute_deadline(
                        wall_now,
                        data.period,
                        offset_dur,
                        anchored_deadline,
                    );
                    let window = data.window.unwrap_or(Window::ZERO);

                    if wall_now >= deadline - window.early && wall_now <= deadline + window.late {
                        let drift = match wall_now.as_nanos().cmp(&deadline.as_nanos()) {
                            Ordering::Greater => Drift::Late(Duration::from_nanos(
                                wall_now.as_nanos() - deadline.as_nanos(),
                            )),
                            Ordering::Less => Drift::Early(Duration::from_nanos(
                                deadline.as_nanos() - wall_now.as_nanos(),
                            )),
                            Ordering::Equal => Drift::OnTime,
                        };
                        Some((drift, deadline))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let state = match immediate_drift {
                    Some((_, deadline)) => TaskState {
                        added_at: now,
                        last_fired: Some(now),
                        last_wall_deadline: Some(deadline),
                        remaining: remaining.map(|r| r.saturating_sub(1)),
                    },
                    None => TaskState {
                        added_at: now,
                        last_fired: None,
                        last_wall_deadline: anchored_deadline,
                        remaining,
                    },
                };

                (state, immediate_drift.map(|(drift, _)| drift))
            }
        };

        self.tasks.push(ScheduledTask { task, state });
        immediate_drift
    }

    pub fn next_tick_deadline(&self) -> Option<MonoInstant> {
        self.soonest_deadline().map(|t| t - self.timer_delay)
    }

    pub fn calculate_next_tick(&self) -> Option<Duration> {
        let now = self.clock.monotonic_now();
        self.soonest_deadline().map(|t| {
            let sleep = t
                .saturating_duration_since(now)
                .saturating_sub(self.timer_delay);
            match self.min_tick_interval {
                Some(min) => sleep.max(min),
                None => sleep,
            }
        })
    }

    fn soonest_deadline(&self) -> Option<MonoInstant> {
        let mut clock_cache: Option<(MonoInstant, WallInstant)> = None;
        let mut soonest: Option<MonoInstant> = None;

        for st in &self.tasks {
            if st.state.remaining == Some(0) {
                continue;
            }

            let mono_deadline = match &st.task.task_type {
                TaskType::Relative(data) => match st.state.last_fired {
                    Some(fired) => fired + data.period,
                    None if data.initial_delay == Duration::ZERO => st.state.added_at,
                    None => st.state.added_at + data.initial_delay,
                },
                TaskType::Absolute(data) => {
                    let (now, wall_now) = *clock_cache
                        .get_or_insert_with(|| (self.clock.monotonic_now(), self.clock.wall_now()));
                    let offset_dur = data.offset.unwrap_or(Duration::ZERO);
                    let deadline = next_absolute_deadline(
                        wall_now,
                        data.period,
                        offset_dur,
                        st.state.last_wall_deadline,
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
                Some(s) => s.min(mono_deadline),
            });
        }

        soonest
    }

    pub fn tick(&mut self) -> TickResult<'_, Ctx> {
        self.running = true;
        self.tasks.retain(|t| t.state.remaining != Some(0));

        let now = self.clock.monotonic_now();
        let mut wall_now: Option<WallInstant> = None;
        let mut result = TickResult {
            fired: vec![],
            missed: vec![],
        };

        for st in &mut self.tasks {
            match &st.task.task_type {
                TaskType::Relative(data) => {
                    tick_relative(data, &st.task, &mut st.state, now, &mut result);
                }
                TaskType::Absolute(data) => {
                    let wall_now = *wall_now.get_or_insert_with(|| self.clock.wall_now());
                    tick_absolute(data, &st.task, &mut st.state, now, wall_now, &mut result);
                }
            }
        }

        result
    }
}
