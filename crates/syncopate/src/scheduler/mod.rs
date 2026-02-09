mod builder;
mod error;
mod handle;
mod r#loop;

pub use builder::*;
pub use error::*;
pub use handle::*;
pub use r#loop::SchedulerLoop;

use crate::task::{TaskConfig, TaskId};

pub(crate) enum Command<Ctx = ()> {
    AddTask {
        config: TaskConfig<Ctx>,
        response: crossbeam::channel::Sender<Result<TaskId, SchedulerError>>,
    },
    Shutdown,
}
