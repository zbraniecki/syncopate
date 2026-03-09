use std::rc::Rc;
use std::time::Duration;
use syncopate::{Repeat, Scheduler, SimClock, TaskBuilder, Window};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn once_after_fires_once_then_gone() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::once_after(Duration::from_millis(500))
        .name("one_shot")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(400));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].task.name.as_deref(), Some("one_shot"));

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn times_3_fires_three_times_then_gone() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100))
        .name("three_shot")
        .repeat(Repeat::Times(3))
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "fire #1 (immediate)");

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "fire #2");

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "fire #3");

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn forever_task_keeps_firing() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100))
        .name("forever")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    for _ in 0..10 {
        clock.advance(Duration::from_millis(100));
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1);
    }

    assert!(scheduler.calculate_next_tick().is_some());
}

#[test]
fn mixed_forever_and_limited_tasks() {
    let (clock, mut scheduler) = make_scheduler();

    let forever_task = TaskBuilder::every(Duration::from_millis(100))
        .name("forever")
        .build()
        .unwrap();
    let limited_task = TaskBuilder::every(Duration::from_millis(100))
        .name("limited")
        .repeat(Repeat::Times(2))
        .build()
        .unwrap();
    scheduler.add_task(forever_task).unwrap();
    scheduler.add_task(limited_task).unwrap();

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 2);

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 2);

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].task.name.as_deref(), Some("forever"));
}

#[test]
fn anchored_once_at_boundary_fires_once() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::once_at(Duration::from_millis(500))
        .window(Window::symmetric(Duration::from_millis(50)))
        .name("anchored_once")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].task.name.as_deref(), Some("anchored_once"));

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn miss_does_not_decrement_remaining() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::once_after(Duration::from_millis(100))
        .name("one_shot")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(250));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);

    assert!(scheduler.calculate_next_tick().is_some());

    clock.advance(Duration::from_millis(50));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}
