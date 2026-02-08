use crate::task::TaskId;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum SchedulerError {
    TaskNotFound(TaskId),
    ChannelFull,
    ShutDown,
    PeriodOutOfBounds(Duration, Duration, Duration),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TaskNotFound(id) => write!(f, "task not found: {:?}", id),
            Self::ChannelFull => write!(f, "command channel full"),
            Self::ShutDown => write!(f, "scheduler is shut down"),
            Self::PeriodOutOfBounds(period, min, max) => write!(
                f,
                "period {:?} is outside bounds [{:?}, {:?}]",
                period, min, max
            ),
        }
    }
}

impl std::error::Error for SchedulerError {}
