mod builder;
mod error;
mod handle;
mod r#loop;
mod plan;

pub use builder::*;
pub use error::*;
pub use handle::*;
pub use plan::*;
pub use r#loop::*;

use crate::task::{TaskConfig, TaskId};

pub(crate) enum Command {
    AddTask {
        config: TaskConfig,
        response: crossbeam::channel::Sender<Result<TaskId, SchedulerError>>,
    },
    Shutdown,
}
