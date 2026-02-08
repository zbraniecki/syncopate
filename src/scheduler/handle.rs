use crate::{
    scheduler::{error::SchedulerError, Command},
    task::{TaskConfig, TaskId},
};
use crossbeam::channel::{bounded, Sender};

/// Cloneable, Send + Sync handle for submitting commands to the scheduler.
#[derive(Clone)]
pub struct SchedulerHandle {
    pub(crate) cmd_tx: Sender<Command>,
}

impl SchedulerHandle {
    /// Add a new task. Returns a TaskId for future reference.
    pub fn add_task(&self, config: TaskConfig) -> Result<TaskId, SchedulerError> {
        let (response_tx, response_rx) = bounded(1);

        self.cmd_tx
            .send(Command::AddTask {
                config,
                response: response_tx,
            })
            .map_err(|_| SchedulerError::ShutDown)?;

        response_rx.recv().map_err(|_| SchedulerError::ShutDown)?
    }

    /// Initiate graceful shutdown.
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
    }
}
