use crate::{
    scheduler::{Command, SchedulerError, TaskConfig},
    task::{TaskExecution, TaskId, TaskMiss, TaskType},
};
use crossbeam::channel::Receiver;
use std::{
    collections::BinaryHeap,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct ScheduledTask<Ctx = ()> {
    id: TaskId,
    config: TaskConfig<Ctx>,
    next_deadline: Instant,
    miss_count: usize,
}

impl<Ctx> ScheduledTask<Ctx> {
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

    fn is_anchored(&self) -> bool {
        match &self.config.task_type {
            TaskType::Periodic { anchored, .. } => *anchored,
        }
    }
}

// Reverse ordering for min-heap behavior
impl<Ctx> PartialEq for ScheduledTask<Ctx> {
    fn eq(&self, other: &Self) -> bool {
        self.next_deadline == other.next_deadline
    }
}

impl<Ctx> Eq for ScheduledTask<Ctx> {}

impl<Ctx> PartialOrd for ScheduledTask<Ctx> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<Ctx> Ord for ScheduledTask<Ctx> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order for min-heap
        other
            .next_deadline
            .cmp(&self.next_deadline)
            .then_with(|| other.config.priority.cmp(&self.config.priority))
    }
}

/// Single-owner scheduler loop. Processes commands and computes plans.
pub struct SchedulerLoop<Ctx = ()> {
    pub(crate) cmd_rx: Option<Receiver<Command<Ctx>>>,
    pub(crate) tasks: BinaryHeap<ScheduledTask<Ctx>>,
    pub(crate) next_task_id: u32,
    pub(crate) next_generation: u32,
    pub(crate) min_period: Duration,
    pub(crate) max_period: Duration,
    pub(crate) sleep_compensation: Duration,
    pub(crate) context: Ctx,
}

impl<Ctx> SchedulerLoop<Ctx> {
    /// Drain pending commands, advance time, and return how long to sleep.
    ///
    /// Tasks are executed via their callbacks during this call, with access to the context.
    pub fn poll(&mut self) -> Duration {
        // Process all pending commands
        while let Some(cmd) = self.cmd_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
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

                // Invoke execution callback if present
                if let Some(callback) = &task.config.on_execute {
                    let drift = if now >= task.next_deadline {
                        now.duration_since(task.next_deadline)
                    } else {
                        Duration::ZERO
                    };

                    let execution = TaskExecution {
                        task_id: task.id,
                        ideal_time: task.next_deadline,
                        actual_time: now,
                        drift,
                        was_late: now > task.next_deadline,
                    };
                    callback(execution, &self.context);
                }

                // Reset miss count and reschedule
                task.miss_count = 0;
                if task.is_anchored() {
                    task.next_deadline += task.period();
                } else {
                    task.next_deadline = now + task.period();
                }
                to_reschedule.push(task);
            } else {
                // Task missed its window
                task.miss_count += 1;

                // Invoke miss callback if present
                if let Some(on_miss) = &task.config.on_miss {
                    let miss = TaskMiss {
                        task_id: task.id,
                        ideal_time: task.next_deadline,
                        window_end,
                        miss_count: task.miss_count,
                    };
                    on_miss(miss, &self.context);
                }

                // Reschedule to next period
                if task.is_anchored() {
                    task.next_deadline += task.period();
                } else {
                    task.next_deadline = now + task.period();
                }
                to_reschedule.push(task);
            }
        }

        // Re-insert rescheduled tasks
        for task in to_reschedule {
            self.tasks.push(task);
        }

        // Compute idle duration
        // Target the ideal deadline minus sleep_compensation to account for
        // OS/runtime sleep overshoot. This way the actual wakeup lands closer
        // to the ideal time. Clamp to window_start at the earliest — waking
        // before the window would just cause an extra poll cycle.
        if let Some(next_task) = self.tasks.peek() {
            let compensated = next_task
                .next_deadline
                .checked_sub(self.sleep_compensation)
                .unwrap_or(next_task.next_deadline);
            let next_wakeup = compensated.max(next_task.window_start());
            next_wakeup.saturating_duration_since(now)
        } else {
            // When no tasks are scheduled, sleep for max_period
            self.max_period
        }
    }

    /// Add a task directly to the scheduler without going through the channel.
    ///
    /// This method is for single-threaded usage where the scheduler and task creation
    /// are on the same thread. It does not require the context to be Send + Sync.
    ///
    /// For multi-threaded usage, use `SchedulerHandle::add_task()` instead.
    pub fn add_task_local(&mut self, config: TaskConfig<Ctx>) -> Result<TaskId, SchedulerError> {
        self.add_task_internal(config)
    }

    fn add_task_internal(&mut self, config: TaskConfig<Ctx>) -> Result<TaskId, SchedulerError> {
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
