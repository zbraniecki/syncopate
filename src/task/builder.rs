use crate::task::{
    AbsoluteTask, MissCallback, MissedTickBehavior, PeriodicSchedule, RelativeTask, Repeat, Task,
    TaskCallback, TaskType, Window,
};
use std::marker::PhantomData;
use std::time::Duration;

pub struct Relative;
pub struct Absolute;

pub struct TaskBuilder<Kind, Ctx = ()> {
    _kind: PhantomData<Kind>,
    period: Duration,
    window: Option<Window>,
    offset: Option<Duration>,
    repeat: Repeat,
    priority: u8,
    name: Option<String>,
    on_execute: Option<TaskCallback<Ctx>>,
    on_missed: Option<MissCallback<Ctx>>,
    schedule: PeriodicSchedule,
    on_miss: MissedTickBehavior,
    initial_delay: Duration,
}

impl<Ctx> TaskBuilder<Relative, Ctx> {
    pub fn every(period: Duration) -> Self {
        Self {
            _kind: PhantomData,
            period,
            window: None,
            offset: None,
            repeat: Repeat::Forever,
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: Duration::ZERO,
        }
    }

    pub fn once_after(delay: Duration) -> Self {
        Self {
            _kind: PhantomData,
            period: delay,
            window: None,
            offset: None,
            repeat: Repeat::Times(1),
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: delay,
        }
    }

    pub fn schedule(mut self, schedule: PeriodicSchedule) -> Self {
        self.schedule = schedule;
        self
    }

    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    pub fn build(self) -> Task<Ctx> {
        Task {
            task_type: TaskType::Relative(RelativeTask {
                period: self.period,
                window: self.window,
                schedule: self.schedule,
                on_miss: self.on_miss,
                initial_delay: self.initial_delay,
            }),
            repeat: self.repeat,
            priority: self.priority,
            name: self.name,
            on_execute: self.on_execute,
            on_missed: self.on_missed,
        }
    }
}

impl<Ctx> TaskBuilder<Absolute, Ctx> {
    pub fn every_absolute(period: Duration) -> Self {
        Self {
            _kind: PhantomData,
            period,
            window: None,
            offset: None,
            repeat: Repeat::Forever,
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: Duration::ZERO,
        }
    }

    pub fn once_at(period: Duration) -> Self {
        Self {
            _kind: PhantomData,
            period,
            window: None,
            offset: None,
            repeat: Repeat::Times(1),
            priority: 0,
            name: None,
            on_execute: None,
            on_missed: None,
            schedule: PeriodicSchedule::default(),
            on_miss: MissedTickBehavior::default(),
            initial_delay: Duration::ZERO,
        }
    }

    pub fn offset(mut self, offset: Duration) -> Self {
        assert!(
            offset < self.period,
            "offset {offset:?} must be less than period {:?}",
            self.period
        );
        self.offset = Some(offset);
        self
    }

    pub fn build(self) -> Task<Ctx> {
        Task {
            task_type: TaskType::Absolute(AbsoluteTask {
                period: self.period,
                offset: self.offset,
                window: self.window,
                on_miss: self.on_miss,
            }),
            repeat: self.repeat,
            priority: self.priority,
            name: self.name,
            on_execute: self.on_execute,
            on_missed: self.on_missed,
        }
    }
}

impl<Kind, Ctx> TaskBuilder<Kind, Ctx> {
    pub fn window(mut self, window: Window) -> Self {
        self.window = Some(window);
        self
    }

    pub fn once(mut self) -> Self {
        self.repeat = Repeat::Times(1);
        self
    }

    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn on_execute(mut self, callback: TaskCallback<Ctx>) -> Self {
        self.on_execute = Some(callback);
        self
    }

    pub fn on_missed(mut self, callback: MissCallback<Ctx>) -> Self {
        self.on_missed = Some(callback);
        self
    }

    pub fn on_miss(mut self, behavior: MissedTickBehavior) -> Self {
        self.on_miss = behavior;
        self
    }
}
