mod deadline;
mod tick;
mod types;

pub use types::{AddTaskError, MissedExecution, TaskExecution, TickResult};

use crate::clock::{Clock, MonoInstant, RealClock};
use crate::task::{Repeat, Task, TaskType};
use deadline::{floor_wall_deadline, next_absolute_deadline};
use std::time::Duration;
use tick::{tick_absolute, tick_relative};
use types::{ScheduledTask, TaskState};

pub struct Scheduler<Ctx = (), C: Clock = RealClock> {
    clock: C,
    timer_delay: Duration,
    min_tick_interval: Option<Duration>,
    tasks: Vec<ScheduledTask<Ctx>>,
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
        }
    }

    pub fn set_timer_delay(&mut self, timer_delay: Duration) {
        self.timer_delay = timer_delay;
    }

    pub fn set_min_tick_interval(&mut self, interval: Option<Duration>) {
        self.min_tick_interval = interval;
    }

    pub fn add_task(&mut self, task: Task<Ctx>) -> Result<(), AddTaskError> {
        let now = self.clock.monotonic_now();

        let last_wall_deadline = match &task.task_type {
            TaskType::Absolute { period, offset, .. } => {
                let wall_now = self.clock.wall_now();
                let offset_dur = offset.unwrap_or(Duration::ZERO);
                let floor = floor_wall_deadline(wall_now, *period, offset_dur);
                match floor {
                    Some(f) if f == wall_now => None,
                    other => other,
                }
            }
            _ => None,
        };

        let remaining = match task.repeat {
            Repeat::Forever => None,
            Repeat::Times(n) => Some(n),
        };

        self.tasks.push(ScheduledTask {
            task,
            state: TaskState {
                added_at: now,
                last_fired: None,
                last_wall_deadline,
                remaining,
            },
        });
        Ok(())
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
        let mut soonest: Option<MonoInstant> = None;

        for st in &self.tasks {
            if st.state.remaining == Some(0) {
                continue;
            }

            let mono_deadline = match &st.task.task_type {
                TaskType::Relative {
                    period,
                    initial_delay,
                    ..
                } => match st.state.last_fired {
                    Some(fired) => fired + *period,
                    None if *initial_delay == Duration::ZERO => st.state.added_at,
                    None => st.state.added_at + *initial_delay,
                },
                TaskType::Absolute { period, offset, .. } => {
                    let now = self.clock.monotonic_now();
                    let wall_now = self.clock.wall_now();
                    let offset_dur = offset.unwrap_or(Duration::ZERO);
                    let deadline = next_absolute_deadline(
                        wall_now,
                        *period,
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
                Some(s) if mono_deadline < s => mono_deadline,
                Some(s) => s,
            });
        }

        soonest
    }

    pub fn tick(&mut self) -> TickResult<'_, Ctx> {
        self.tasks.retain(|t| t.state.remaining != Some(0));

        let now = self.clock.monotonic_now();
        let wall_now = self.clock.wall_now();
        let mut fired = vec![];
        let mut missed = vec![];

        for st in &mut self.tasks {
            match &st.task.task_type {
                TaskType::Relative { .. } => {
                    tick_relative(&st.task, &mut st.state, now, &mut fired, &mut missed);
                }
                TaskType::Absolute { .. } => {
                    tick_absolute(
                        &st.task,
                        &mut st.state,
                        now,
                        wall_now,
                        &mut fired,
                        &mut missed,
                    );
                }
            }
        }

        TickResult { fired, missed }
    }
}
