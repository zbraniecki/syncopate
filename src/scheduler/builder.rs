use crate::scheduler::{SchedulerHandle, SchedulerLoop};
use crossbeam::channel::bounded;
use std::{collections::BinaryHeap, time::Duration};

pub struct SchedulerBuilder {
    min_period: Duration,
    max_period: Duration,
}

impl SchedulerBuilder {
    pub fn new() -> Self {
        Self {
            min_period: Duration::from_millis(1),
            max_period: Duration::from_secs(3600),
        }
    }

    pub fn min_period(mut self, period: Duration) -> Self {
        self.min_period = period;
        self
    }

    pub fn max_period(mut self, period: Duration) -> Self {
        self.max_period = period;
        self
    }

    pub fn build(self) -> (SchedulerHandle, SchedulerLoop) {
        let (cmd_tx, cmd_rx) = bounded(256);

        let handle = SchedulerHandle { cmd_tx };

        let scheduler_loop = SchedulerLoop {
            cmd_rx,
            tasks: BinaryHeap::new(),
            next_task_id: 0,
            next_generation: 1,
            min_period: self.min_period,
            max_period: self.max_period,
        };

        (handle, scheduler_loop)
    }
}

impl Default for SchedulerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
