use crate::task::{Task, TaskType};
use std::time::Duration;

struct ScheduledTask {
    task: Task,
    start_offset: Duration, // when this task's cycle started (relative to scheduler start)
}

impl ScheduledTask {
    fn period(&self) -> Duration {
        match &self.task.task_type {
            TaskType::Periodic { period } => *period,
        }
    }

    /// Calculate time until next execution given current elapsed time.
    /// If we're exactly on a firing boundary, returns the full period (time to next fire).
    fn time_until_next(&self, elapsed: Duration) -> Duration {
        let since_start = elapsed.saturating_sub(self.start_offset);
        let period = self.period();

        if period.is_zero() {
            return Duration::ZERO;
        }

        let period_nanos = period.as_nanos();
        let since_start_nanos = since_start.as_nanos();
        let remainder = since_start_nanos % period_nanos;

        if remainder == 0 {
            period // on a boundary (including start), wait full period
        } else {
            Duration::from_nanos((period_nanos - remainder) as u64)
        }
    }

    /// Check if task should execute at the given elapsed time.
    /// True when we're exactly on a period boundary (but not at the very start).
    fn is_ready(&self, elapsed: Duration) -> bool {
        let since_start = elapsed.saturating_sub(self.start_offset);
        let period = self.period();

        if period.is_zero() || since_start.is_zero() {
            return false;
        }

        let period_nanos = period.as_nanos();
        let since_start_nanos = since_start.as_nanos();

        since_start_nanos % period_nanos == 0
    }
}

#[derive(Default)]
pub struct Scheduler {
    tasks: Vec<ScheduledTask>,
    elapsed: Duration, // total time since scheduler start
}

impl Scheduler {
    pub fn add_task(&mut self, task: Task) {
        self.tasks.push(ScheduledTask {
            task,
            start_offset: self.elapsed, // task cycle starts from now
        });
    }

    pub fn calculate_next_tick(&self) -> Option<Duration> {
        self.tasks
            .iter()
            .map(|t| t.time_until_next(self.elapsed))
            .min()
    }

    /// Advance time by the given duration.
    /// Returns tasks that are ready to execute.
    pub fn tick(&mut self, duration: Duration) -> Vec<&Task> {
        self.elapsed += duration;

        self.tasks
            .iter()
            .filter(|t| t.is_ready(self.elapsed))
            .map(|t| &t.task)
            .collect()
    }
}
