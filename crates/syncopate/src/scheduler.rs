use crate::task::{Task, TaskType};
use std::time::Duration;

struct ScheduledTask {
    task: Task,
    next_fire: Duration, // absolute time from scheduler start when this task next fires
}

impl ScheduledTask {
    fn period(&self) -> Duration {
        match &self.task.task_type {
            TaskType::Periodic { period } => *period,
        }
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
        self.next_fire += self.period();
    }
}

#[derive(Default)]
pub struct Scheduler {
    tasks: Vec<ScheduledTask>,
    elapsed: Duration, // total time since scheduler start
}

impl Scheduler {
    /// Add a periodic task to the scheduler.
    ///
    /// # Arguments
    /// * `task` - The task to add
    /// * `current_epoch_nanos` - Current time in nanoseconds within the task's period.
    ///   For anchored tasks, this determines when the first tick occurs (aligned to period boundaries).
    ///   For non-anchored tasks, this parameter is ignored.
    ///
    ///   Example: if the task has a 1-second period and it's currently 300ms
    ///   into the current second, pass 300_000_000.
    pub fn add_task(&mut self, task: Task, current_epoch_nanos: u128) {
        let next_fire = if task.anchored {
            let period = task.task_type.period();
            let period_nanos = period.as_nanos();

            // Calculate offset within the current period
            let offset_in_period = current_epoch_nanos % period_nanos;

            // Calculate time until next boundary
            let nanos_until_boundary = if offset_in_period == 0 {
                period_nanos // At boundary, wait for next one
            } else {
                period_nanos - offset_in_period
            };

            self.elapsed + Duration::from_nanos(nanos_until_boundary as u64)
        } else {
            // Non-anchored tasks fire one period from now
            self.elapsed + task.task_type.period()
        };

        self.tasks.push(ScheduledTask { task, next_fire });
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

        // Find indices of ready tasks
        let ready_indices: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_ready(self.elapsed))
            .map(|(i, _)| i)
            .collect();

        // Advance ready tasks
        for &idx in &ready_indices {
            self.tasks[idx].advance();
        }

        // Collect task references
        ready_indices
            .iter()
            .map(|&idx| &self.tasks[idx].task)
            .collect()
    }
}

impl TaskType {
    fn period(&self) -> Duration {
        match self {
            TaskType::Periodic { period } => *period,
        }
    }
}
