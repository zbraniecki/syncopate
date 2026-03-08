use std::rc::Rc;
use std::time::Duration;
use syncopate::{
    Window,
    scheduler::{Drift, Scheduler},
    system_time::SimClock,
    task::TaskBuilder,
};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn calculate_next_tick_single_task() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // With Immediate initial tick, the first deadline is at t=0.
    assert_eq!(scheduler.calculate_next_tick(), Some(Duration::ZERO));

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // After consuming, next deadline is at 500ms.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn calculate_next_tick_returns_shortest_period() {
    let (_clock, mut scheduler) = make_scheduler();

    let fast = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("fast")
        .build()
        .unwrap();
    let slow = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
        .name("slow")
        .build()
        .unwrap();
    scheduler.add_task(fast).unwrap();
    scheduler.add_task(slow).unwrap();

    // With Immediate initial tick, both tasks have deadline at t=0.
    assert_eq!(scheduler.calculate_next_tick(), Some(Duration::ZERO));

    // Consume the immediate fires at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 2);

    // After consuming, the soonest deadline is fast's at 100ms.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(100))
    );
}

#[test]
fn calculate_next_tick_no_tasks_returns_none() {
    let (_clock, scheduler) = make_scheduler();
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn task_fires_when_deadline_reached() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Before deadline (500ms): nothing fires.
    clock.advance(Duration::from_millis(499));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    // At the deadline: fires.
    clock.advance(Duration::from_millis(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
}

#[test]
fn task_fires_when_past_deadline_within_late_window() {
    let (clock, mut scheduler) = make_scheduler();

    // Late window of 100ms: task may fire up to 100ms after the deadline.
    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // 50ms past the deadline (500ms) — within the 100ms late window, so it fires.
    clock.advance(Duration::from_millis(550));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(50))
    );
    assert_eq!(result.missed.len(), 0);
}

#[test]
fn task_fires_early_within_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    // Early window of 100ms: task may fire up to 100ms before the deadline.
    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::from_millis(100), Duration::ZERO),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // 80ms before the deadline (420ms elapsed) — within the 100ms early window.
    clock.advance(Duration::from_millis(420));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Early(Duration::from_millis(80))
    );
    assert_eq!(result.missed.len(), 0);
}

#[test]
fn task_missed_when_past_late_window() {
    let (clock, mut scheduler) = make_scheduler();

    // Zero window: task must fire exactly at the deadline.
    let task = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // 50ms past the deadline (500ms) with zero window — missed.
    clock.advance(Duration::from_millis(550));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(
        result.missed[0].deadlines_missed,
        vec![Duration::from_millis(50)]
    );
}

#[test]
fn task_not_yet_due_before_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::from_millis(100), Duration::ZERO),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // 350ms elapsed — still 50ms before the early window opens at 400ms.
    clock.advance(Duration::from_millis(350));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);
}

#[test]
fn multiple_tasks_all_fire_at_shared_deadline() {
    let (clock, mut scheduler) = make_scheduler();

    for name in ["a", "b", "c"] {
        let task = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
            .name(name)
            .build()
            .unwrap();
        scheduler.add_task(task).unwrap();
    }

    // Consume the immediate fires at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 3);

    // All three fire again at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 3);
}
