use super::{MissCallback, TaskCallback, TaskType};

/// Configuration for a new task.
#[derive(Clone)]
pub struct TaskConfig<Ctx = ()> {
    /// What kind of task (periodic or one-shot).
    pub task_type: TaskType,

    /// Priority level (0 = highest).
    pub priority: u8,

    /// Optional human-readable name for debugging and stats.
    pub name: Option<String>,

    /// Optional callback invoked when the task is executed.
    /// The callback receives timing information including drift and a reference to the context.
    pub on_execute: Option<TaskCallback<Ctx>>,

    /// Optional callback invoked when the task misses its execution window.
    /// The callback receives miss count and timing information and a reference to the context.
    pub on_miss: Option<MissCallback<Ctx>>,
}

impl<Ctx> std::fmt::Debug for TaskConfig<Ctx> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskConfig")
            .field("task_type", &self.task_type)
            .field("priority", &self.priority)
            .field("name", &self.name)
            .field("on_execute", &self.on_execute.as_ref().map(|_| "Fn"))
            .field("on_miss", &self.on_miss.as_ref().map(|_| "Fn"))
            .finish()
    }
}

impl<Ctx> TaskConfig<Ctx> {
    /// Register an execution callback for this task.
    ///
    /// The callback will be invoked synchronously during `poll()` when the task is due.
    /// Keep callbacks fast to avoid blocking the scheduler. For async work, spawn tasks
    /// from within the callback.
    ///
    /// The callback receives the execution information and a reference to the scheduler's context.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use syncopate::task::{TaskConfig, TaskType};
    /// use std::time::Duration;
    ///
    /// let config = TaskConfig::<()> {
    ///     task_type: TaskType::Periodic {
    ///         period: Duration::from_secs(1),
    ///         window_before: Duration::from_millis(10),
    ///         window_after: Duration::from_millis(10),
    ///     },
    ///     priority: 0,
    ///     name: Some("heartbeat".into()),
    ///     on_execute: None,
    ///     on_miss: None,
    /// }
    /// .with_executor(|exec, _ctx| {
    ///     println!("Task executed with drift: {:?}", exec.drift);
    /// });
    /// ```
    pub fn with_executor<F>(mut self, f: F) -> Self
    where
        F: Fn(super::TaskExecution, &Ctx) + Send + Sync + 'static,
    {
        self.on_execute = Some(std::sync::Arc::new(f));
        self
    }

    /// Register a miss callback for this task.
    ///
    /// The callback will be invoked synchronously during `poll()` when the task misses
    /// its execution window.
    ///
    /// The callback receives the miss information and a reference to the scheduler's context.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use syncopate::task::{TaskConfig, TaskType};
    /// use std::time::Duration;
    ///
    /// let config = TaskConfig::<()> {
    ///     task_type: TaskType::Periodic {
    ///         period: Duration::from_secs(1),
    ///         window_before: Duration::from_millis(10),
    ///         window_after: Duration::from_millis(10),
    ///     },
    ///     priority: 0,
    ///     name: Some("heartbeat".into()),
    ///     on_execute: None,
    ///     on_miss: None,
    /// }
    /// .with_miss_handler(|miss, _ctx| {
    ///     eprintln!("Task missed {} times!", miss.miss_count);
    /// });
    /// ```
    pub fn with_miss_handler<F>(mut self, f: F) -> Self
    where
        F: Fn(super::TaskMiss, &Ctx) + Send + Sync + 'static,
    {
        self.on_miss = Some(std::sync::Arc::new(f));
        self
    }
}
