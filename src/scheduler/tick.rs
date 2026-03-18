use super::deadline::{calculate_drift, missed_offsets};
use super::types::{MissedExecution, TaskExecution, TaskState};
use crate::clock::{MonoInstant, WallInstant};
use crate::task::{Drift, MissedTickBehavior, PeriodicSchedule, Task, TaskType, Window};
use std::cmp::Ordering;
use std::time::Duration;

pub(crate) fn tick_relative<'a, Ctx>(
    task: &'a Task<Ctx>,
    state: &mut TaskState,
    now: MonoInstant,
    fired: &mut Vec<TaskExecution<'a, Ctx>>,
    missed: &mut Vec<MissedExecution<'a, Ctx>>,
) {
    let TaskType::Relative {
        period,
        window,
        schedule,
        on_miss,
        initial_delay,
    } = &task.task_type
    else {
        return;
    };
    let (period, window, schedule, on_miss, initial_delay) = (
        *period,
        window.unwrap_or(Window::ZERO),
        *schedule,
        *on_miss,
        *initial_delay,
    );

    let next_deadline = match state.last_fired {
        Some(fired) => fired + period,
        None if initial_delay == Duration::ZERO => state.added_at,
        None => state.added_at + initial_delay,
    };

    let window_start = next_deadline - window.early;
    let window_end = next_deadline + window.late;

    if now < window_start {
        return;
    }

    let drift = calculate_drift(now, next_deadline);

    if now <= window_end {
        state.last_fired = Some(match schedule {
            PeriodicSchedule::FixedRate => next_deadline,
            PeriodicSchedule::FixedDelay => now,
        });
        if let Some(ref mut r) = state.remaining {
            *r = r.saturating_sub(1);
        }
        fired.push(TaskExecution { task, drift });
    } else if schedule == PeriodicSchedule::FixedDelay {
        state.last_fired = Some(now);
        if let Some(ref mut r) = state.remaining {
            *r = r.saturating_sub(1);
        }
        fired.push(TaskExecution {
            task,
            drift: Drift::Late(now - next_deadline),
        });
    } else {
        let elapsed_ns = (now - next_deadline).as_nanos();
        let period_ns = period.as_nanos();
        let periods_elapsed = elapsed_ns / period_ns;
        let count = periods_elapsed as u32 + 1;

        let latest_deadline = next_deadline + period * (periods_elapsed as u32);
        let latest_window_start = latest_deadline - window.early;
        let latest_window_end = latest_deadline + window.late;
        let latest_in_window = now >= latest_window_start && now <= latest_window_end;

        if latest_in_window {
            let missed_count = count - 1;
            if missed_count > 0 {
                missed.push(MissedExecution {
                    task,
                    deadlines_missed: missed_offsets(elapsed_ns, period_ns, missed_count),
                });
            }
            state.last_fired = Some(match schedule {
                PeriodicSchedule::FixedRate => latest_deadline,
                PeriodicSchedule::FixedDelay => now,
            });
            if let Some(ref mut r) = state.remaining {
                *r = r.saturating_sub(1);
            }
            fired.push(TaskExecution {
                task,
                drift: calculate_drift(now, latest_deadline),
            });
        } else {
            match on_miss {
                MissedTickBehavior::RunLatest => {
                    if count > 1 {
                        missed.push(MissedExecution {
                            task,
                            deadlines_missed: missed_offsets(elapsed_ns, period_ns, count - 1),
                        });
                    }
                    state.last_fired = Some(latest_deadline);
                    if let Some(ref mut r) = state.remaining {
                        *r = r.saturating_sub(1);
                    }
                    fired.push(TaskExecution {
                        task,
                        drift: Drift::Late(now - latest_deadline),
                    });
                }
                MissedTickBehavior::Burst { max } => {
                    let fire_count = max.map_or(count, |m| count.min(m));
                    let skip_count = count - fire_count;
                    if skip_count > 0 {
                        missed.push(MissedExecution {
                            task,
                            deadlines_missed: missed_offsets(elapsed_ns, period_ns, skip_count),
                        });
                    }
                    for i in skip_count..count {
                        let offset = period_ns * i as u128;
                        let lateness = elapsed_ns - offset;
                        fired.push(TaskExecution {
                            task,
                            drift: Drift::Late(Duration::from_nanos(lateness as u64)),
                        });
                    }
                    state.last_fired = Some(next_deadline + period * (count - 1));
                    if let Some(ref mut r) = state.remaining {
                        *r = r.saturating_sub(fire_count);
                    }
                }
                MissedTickBehavior::Skip => {
                    missed.push(MissedExecution {
                        task,
                        deadlines_missed: missed_offsets(elapsed_ns, period_ns, count),
                    });
                    state.last_fired = Some(latest_deadline);
                }
            }
        }
    }
}

pub(crate) fn tick_absolute<'a, Ctx>(
    task: &'a Task<Ctx>,
    state: &mut TaskState,
    now: MonoInstant,
    wall_now: WallInstant,
    fired: &mut Vec<TaskExecution<'a, Ctx>>,
    missed: &mut Vec<MissedExecution<'a, Ctx>>,
) {
    let TaskType::Absolute {
        period,
        offset,
        window,
        on_miss,
    } = &task.task_type
    else {
        return;
    };
    let (period, offset_dur, window, on_miss) = (
        *period,
        offset.unwrap_or(Duration::ZERO),
        window.unwrap_or(Window::ZERO),
        *on_miss,
    );

    let deadline = super::deadline::next_absolute_deadline(
        wall_now,
        period,
        offset_dur,
        state.last_wall_deadline,
    );

    let window_start = deadline - window.early;
    let window_end = deadline + window.late;

    if wall_now < window_start {
        return;
    }

    if wall_now <= window_end {
        let drift = match wall_now.cmp(&deadline) {
            Ordering::Greater => Drift::Late(wall_now - deadline),
            Ordering::Less => Drift::Early(deadline - wall_now),
            Ordering::Equal => Drift::OnTime,
        };
        state.last_wall_deadline = Some(deadline);
        state.last_fired = Some(now);
        if let Some(ref mut r) = state.remaining {
            *r = r.saturating_sub(1);
        }
        fired.push(TaskExecution { task, drift });
    } else {
        let elapsed = wall_now - deadline;
        let periods_elapsed = elapsed.as_nanos() / period.as_nanos();
        let count = periods_elapsed as u32 + 1;

        let latest_wall_deadline = deadline + period * periods_elapsed as u32;
        let latest_in_window = wall_now >= latest_wall_deadline - window.early
            && wall_now <= latest_wall_deadline + window.late;

        if latest_in_window {
            let missed_count = count - 1;
            if missed_count > 0 {
                missed.push(MissedExecution {
                    task,
                    deadlines_missed: missed_offsets(
                        elapsed.as_nanos(),
                        period.as_nanos(),
                        missed_count,
                    ),
                });
            }
            let drift = match wall_now.cmp(&latest_wall_deadline) {
                Ordering::Greater => Drift::Late(wall_now - latest_wall_deadline),
                Ordering::Less => Drift::Early(latest_wall_deadline - wall_now),
                Ordering::Equal => Drift::OnTime,
            };
            state.last_wall_deadline = Some(latest_wall_deadline);
            state.last_fired = Some(now);
            if let Some(ref mut r) = state.remaining {
                *r = r.saturating_sub(1);
            }
            fired.push(TaskExecution { task, drift });
        } else {
            match on_miss {
                MissedTickBehavior::RunLatest => {
                    if count > 1 {
                        missed.push(MissedExecution {
                            task,
                            deadlines_missed: missed_offsets(
                                elapsed.as_nanos(),
                                period.as_nanos(),
                                count - 1,
                            ),
                        });
                    }
                    state.last_wall_deadline = Some(latest_wall_deadline);
                    state.last_fired = Some(now);
                    if let Some(ref mut r) = state.remaining {
                        *r = r.saturating_sub(1);
                    }
                    fired.push(TaskExecution {
                        task,
                        drift: Drift::Late(wall_now - latest_wall_deadline),
                    });
                }
                MissedTickBehavior::Burst { max } => {
                    let fire_count = max.map_or(count, |m| count.min(m));
                    let skip_count = count - fire_count;
                    if skip_count > 0 {
                        missed.push(MissedExecution {
                            task,
                            deadlines_missed: missed_offsets(
                                elapsed.as_nanos(),
                                period.as_nanos(),
                                skip_count,
                            ),
                        });
                    }
                    for i in skip_count..count {
                        let fired_deadline = deadline + period * i;
                        fired.push(TaskExecution {
                            task,
                            drift: Drift::Late(wall_now - fired_deadline),
                        });
                    }
                    state.last_wall_deadline = Some(latest_wall_deadline);
                    state.last_fired = Some(now);
                    if let Some(ref mut r) = state.remaining {
                        *r = r.saturating_sub(fire_count);
                    }
                }
                MissedTickBehavior::Skip => {
                    missed.push(MissedExecution {
                        task,
                        deadlines_missed: missed_offsets(
                            elapsed.as_nanos(),
                            period.as_nanos(),
                            count,
                        ),
                    });
                    state.last_wall_deadline = Some(latest_wall_deadline);
                    state.last_fired = Some(now);
                }
            }
        }
    }
}
