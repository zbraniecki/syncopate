use crate::{
    scheduler::{Command, error::SchedulerError},
    task::{TaskConfig, TaskId},
};
use crossbeam::channel::{Sender, bounded};

/// Cloneable, Send + Sync handle for submitting commands to the scheduler.
///
/// This handle allows adding tasks from multiple threads. The context type `Ctx`
/// must implement `Send + Sync + 'static`.
#[derive(Clone)]
pub struct SchedulerHandle<Ctx = ()> {
    pub(crate) cmd_tx: Sender<Command<Ctx>>,
}

impl<Ctx> SchedulerHandle<Ctx>
where
    Ctx: Send + Sync + 'static,
{
    /// Add a new task from any thread. Returns a TaskId for future reference.
    ///
    /// This method requires the task configuration (including callbacks) to be Send + Sync.
    pub fn add_task(&self, config: TaskConfig<Ctx>) -> Result<TaskId, SchedulerError> {
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
