mod instant;
mod real;
mod reference;
mod sim;

pub use instant::{MonoInstant, WallInstant};
pub use real::RealClock;
pub use reference::TimeReference;
pub use sim::SimClock;

use std::rc::Rc;
use std::sync::Arc;

pub trait Clock {
    fn monotonic_now(&self) -> MonoInstant;
    fn wall_now(&self) -> WallInstant;
}

impl<C: Clock> Clock for Rc<C> {
    fn monotonic_now(&self) -> MonoInstant {
        (**self).monotonic_now()
    }
    fn wall_now(&self) -> WallInstant {
        (**self).wall_now()
    }
}

impl<C: Clock> Clock for Arc<C> {
    fn monotonic_now(&self) -> MonoInstant {
        (**self).monotonic_now()
    }
    fn wall_now(&self) -> WallInstant {
        (**self).wall_now()
    }
}
