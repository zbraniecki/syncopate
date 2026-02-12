use crate::scheduler::{SchedulerHandle, SchedulerLoop};
use crossbeam::channel::bounded;
use std::{collections::BinaryHeap, time::Duration};

pub struct SchedulerBuilder<Ctx = ()> {
    min_period: Duration,
    max_period: Duration,
    sleep_compensation: Duration,
    context: Ctx,
}

impl Default for SchedulerBuilder<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerBuilder<()> {
    /// Create a new scheduler builder with default settings and unit context.
    pub fn new() -> Self {
        Self {
            min_period: Duration::from_millis(1),
            max_period: Duration::from_secs(3600),
            sleep_compensation: Duration::ZERO,
            context: (),
        }
    }
}

impl<Ctx> SchedulerBuilder<Ctx> {
    /// Set the minimum allowed task period.
    pub fn min_period(mut self, period: Duration) -> Self {
        self.min_period = period;
        self
    }

    /// Set the maximum allowed task period.
    pub fn max_period(mut self, period: Duration) -> Self {
        self.max_period = period;
        self
    }

    /// Set sleep compensation to account for OS/runtime sleep overshoot.
    ///
    /// The scheduler will request wakeups this much earlier than the ideal
    /// deadline, so that the actual wakeup (after OS overshoot) lands closer
    /// to the target time. For example, if tokio typically overshoots by 2ms,
    /// setting this to 2ms will improve timing accuracy.
    pub fn sleep_compensation(mut self, duration: Duration) -> Self {
        self.sleep_compensation = duration;
        self
    }

    /// Provide a custom context that will be passed to all task callbacks.
    ///
    /// The context can be used to share state between tasks and the application.
    /// For single-threaded usage, the context can use `Rc<RefCell<T>>`.
    /// For multi-threaded usage with `build()`, the context must implement `Send + Sync`,
    /// typically using `Arc<Mutex<T>>` or similar.
    pub fn with_context<NewCtx>(self, context: NewCtx) -> SchedulerBuilder<NewCtx> {
        SchedulerBuilder {
            min_period: self.min_period,
            max_period: self.max_period,
            sleep_compensation: self.sleep_compensation,
            context,
        }
    }

    /// Build the scheduler for multi-threaded use.
    ///
    /// Returns a handle that can be cloned and sent across threads, and the scheduler loop
    /// that should run in a dedicated task/thread.
    ///
    /// The context must implement `Send + Sync + 'static` for this method.
    pub fn build(self) -> (SchedulerHandle<Ctx>, SchedulerLoop<Ctx>)
    where
        Ctx: Send + Sync + 'static,
    {
        let (cmd_tx, cmd_rx) = bounded(256);

        let handle = SchedulerHandle { cmd_tx };

        let scheduler_loop = SchedulerLoop {
            cmd_rx: Some(cmd_rx),
            tasks: BinaryHeap::new(),
            next_task_id: 0,
            next_generation: 1,
            min_period: self.min_period,
            max_period: self.max_period,
            sleep_compensation: self.sleep_compensation,
            context: self.context,
        };

        (handle, scheduler_loop)
    }

    /// Build the scheduler for single-threaded use without a handle.
    ///
    /// Use this when you don't need to add tasks from other threads.
    /// This does not require the context to implement `Send + Sync`.
    ///
    /// Tasks can be added directly via `SchedulerLoop::add_task_local()`.
    pub fn build_local(self) -> SchedulerLoop<Ctx> {
        SchedulerLoop {
            cmd_rx: None,
            tasks: BinaryHeap::new(),
            next_task_id: 0,
            next_generation: 1,
            min_period: self.min_period,
            max_period: self.max_period,
            sleep_compensation: self.sleep_compensation,
            context: self.context,
        }
    }
}
