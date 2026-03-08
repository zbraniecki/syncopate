use std::rc::Rc;
use std::time::Duration;
use syncopate::{Repeat, Window, scheduler::Scheduler, system_time::SimClock, task::TaskBuilder};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn once_after_fires_once_then_gone() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::once_after(Duration::from_millis(500), Window::ZERO)
        .name("one_shot")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Before deadline: nothing fires.
    clock.advance(Duration::from_millis(400));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    // At deadline: fires.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].task.name.as_deref(), Some("one_shot"));

    // Next tick: task is evicted, nothing fires, no tasks left.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn times_3_fires_three_times_then_gone() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("three_shot")
        .repeat(Repeat::Times(3))
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire #1: immediate tick at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "fire #1 (immediate)");

    // Fire #2 at t=100ms.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "fire #2");

    // Fire #3 at t=200ms.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "fire #3");

    // 4th tick: task is gone.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn forever_task_keeps_firing() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("forever")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    for _ in 0..10 {
        clock.advance(Duration::from_millis(100));
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1);
    }

    // Still schedulable.
    assert!(scheduler.calculate_next_tick().is_some());
}

#[test]
fn mixed_forever_and_limited_tasks() {
    let (clock, mut scheduler) = make_scheduler();

    let forever_task = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("forever")
        .build()
        .unwrap();
    let limited_task = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("limited")
        .repeat(Repeat::Times(2))
        .build()
        .unwrap();
    scheduler.add_task(forever_task).unwrap();
    scheduler.add_task(limited_task).unwrap();

    // Tick 0 (immediate): both fire.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 2);

    // Tick 1 at t=100ms: both fire (limited has 1 remaining before this tick).
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 2);

    // Tick 2 at t=200ms: only forever fires, limited is evicted.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].task.name.as_deref(), Some("forever"));
}

#[test]
fn anchored_once_at_boundary_fires_once() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::at_boundary(
        Duration::from_millis(500),
        Window::symmetric(Duration::from_millis(50)),
    )
    .name("anchored_once")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire at the 500ms boundary.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].task.name.as_deref(), Some("anchored_once"));

    // Next tick: task is evicted.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn miss_does_not_decrement_remaining() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::once_after(Duration::from_millis(100), Window::ZERO)
        .name("one_shot")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Miss the deadline (zero window, arrive between deadlines).
    // Advance to 250ms — deadline at 100ms is missed, deadline at 200ms is missed,
    // and 250ms doesn't coincide with any deadline.
    clock.advance(Duration::from_millis(250));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);

    // Task should still be schedulable (miss didn't consume the fire).
    assert!(scheduler.calculate_next_tick().is_some());

    // Next period fires (deadline at 300ms, arrive exactly at 300ms).
    clock.advance(Duration::from_millis(50));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Now it's gone.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(scheduler.calculate_next_tick(), None);
}
