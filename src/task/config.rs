use super::TaskType;

/// Configuration for a new task.
#[derive(Debug, Clone)]
pub struct TaskConfig {
    /// What kind of task (periodic or one-shot).
    pub task_type: TaskType,

    /// Priority level (0 = highest).
    pub priority: u8,

    /// Optional human-readable name for debugging and stats.
    pub name: Option<String>,
}
