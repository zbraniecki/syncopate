use crate::{
    scheduler::{Command, SchedulerError, TaskConfig, WakeupPlan},
    task::{TaskId, TaskType},
};
use crossbeam::channel::Receiver;
use std::{
    collections::BinaryHeap,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct ScheduledTask {
    id: TaskId,
    config: TaskConfig,
    next_deadline: Instant,
    miss_count: usize,
}

impl ScheduledTask {
    fn window_start(&self) -> Instant {
        match &self.config.task_type {
            TaskType::Periodic { window_before, .. } => self
                .next_deadline
                .checked_sub(*window_before)
                .unwrap_or(self.next_deadline),
        }
    }

    fn window_end(&self) -> Instant {
        match &self.config.task_type {
            TaskType::Periodic { window_after, .. } => self
                .next_deadline
                .checked_add(*window_after)
                .unwrap_or(self.next_deadline),
        }
    }

    fn period(&self) -> Duration {
        match &self.config.task_type {
            TaskType::Periodic { period, .. } => *period,
        }
    }
}

// Reverse ordering for min-heap behavior
impl PartialEq for ScheduledTask {
    fn eq(&self, other: &Self) -> bool {
        self.next_deadline == other.next_deadline
    }
}

impl Eq for ScheduledTask {}

impl PartialOrd for ScheduledTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order for min-heap
        other
            .next_deadline
            .cmp(&self.next_deadline)
            .then_with(|| other.config.priority.cmp(&self.config.priority))
    }
}

/// A task that is due for execution.
#[derive(Debug, Clone)]
pub struct DueTask {
    /// The task's unique identifier.
    pub id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// The task's priority level (0 = highest).
    pub priority: u8,
}

/// A task that missed its execution window.
#[derive(Debug, Clone)]
pub struct MissedTask {
    /// The task's unique identifier.
    pub id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// The end of the acceptable execution window.
    pub window_end: Instant,

    /// Number of consecutive misses for this task.
    pub miss_count: usize,
}

/// Single-owner scheduler loop. Processes commands and computes plans.
pub struct SchedulerLoop {
    pub(crate) cmd_rx: Receiver<Command>,
    pub(crate) tasks: BinaryHeap<ScheduledTask>,
    pub(crate) next_task_id: u32,
    pub(crate) next_generation: u32,
    pub(crate) min_period: Duration,
    pub(crate) max_period: Duration,
}

impl SchedulerLoop {
    /// Drain pending commands, advance time, compute the next wakeup plan.
    pub fn poll(&mut self) -> WakeupPlan {
        // Process all pending commands
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Command::AddTask { config, response } => {
                    let result = self.add_task_internal(config);
                    let _ = response.send(result);
                }
                Command::Shutdown => {
                    // For now, just acknowledge shutdown
                    break;
                }
            }
        }

        let now = Instant::now();
        let mut due_tasks = Vec::new();
        let mut missed_tasks = Vec::new();
        let mut to_reschedule = Vec::new();

        // Collect due and missed tasks
        while let Some(task) = self.tasks.peek() {
            let window_start = task.window_start();
            let window_end = task.window_end();

            if now < window_start {
                // Task not ready yet
                break;
            }

            // Remove task from heap for processing
            let mut task = self.tasks.pop().unwrap();

            if now <= window_end {
                // Task is due (within its window)
                due_tasks.push(DueTask {
                    id: task.id,
                    ideal_time: task.next_deadline,
                    priority: task.config.priority,
                });

                // Reset miss count and reschedule
                task.miss_count = 0;
                task.next_deadline += task.period();
                to_reschedule.push(task);
            } else {
                // Task missed its window
                task.miss_count += 1;

                missed_tasks.push(MissedTask {
                    id: task.id,
                    ideal_time: task.next_deadline,
                    window_end,
                    miss_count: task.miss_count,
                });

                // Reschedule to next period
                task.next_deadline += task.period();
                to_reschedule.push(task);
            }
        }

        // Re-insert rescheduled tasks
        for task in to_reschedule {
            self.tasks.push(task);
        }

        // Compute idle duration and next wakeup
        let (idle_duration, next_wakeup) = if let Some(next_task) = self.tasks.peek() {
            let next_wakeup = next_task.window_start();
            let idle = next_wakeup.saturating_duration_since(now);
            (idle, Some(next_wakeup))
        } else {
            // When no tasks are scheduled, sleep for max_period
            (self.max_period, None)
        };

        WakeupPlan {
            idle_duration,
            next_wakeup,
            due_tasks,
            missed_tasks,
        }
    }

    /// Mark tasks as completed. For periodic tasks, they're already rescheduled in poll().
    pub fn mark_completed(&mut self, _tasks: &[TaskId]) {
        // In this simple implementation, tasks are automatically rescheduled
        // in poll(). This method is provided for API completeness.
    }

    fn add_task_internal(&mut self, config: TaskConfig) -> Result<TaskId, SchedulerError> {
        let id = TaskId::new(self.next_task_id, self.next_generation);
        self.next_task_id = self.next_task_id.wrapping_add(1);

        let now = Instant::now();
        let next_deadline = match &config.task_type {
            TaskType::Periodic { period, .. } => {
                if *period < self.min_period {
                    return Err(SchedulerError::PeriodOutOfBounds(
                        *period,
                        self.min_period,
                        self.max_period,
                    ));
                }
                if *period > self.max_period {
                    return Err(SchedulerError::PeriodOutOfBounds(
                        *period,
                        self.min_period,
                        self.max_period,
                    ));
                }
                now + *period
            }
        };

        let task = ScheduledTask {
            id,
            config,
            next_deadline,
            miss_count: 0,
        };

        self.tasks.push(task);

        Ok(id)
    }
}
