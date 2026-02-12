use std::time::Duration;

pub enum TaskType {
    Periodic { period: Duration },
}

pub type TaskCallback<Ctx> = fn(&Ctx);
pub type MissCallback<Ctx> = fn(&Ctx);

pub struct Task<Ctx = ()> {
    pub task_type: TaskType,
    pub anchored: bool,
    pub priority: u8,
    pub name: Option<String>,
    pub on_execute: Option<TaskCallback<Ctx>>,
    pub on_miss: Option<MissCallback<Ctx>>,
}
