use std::rc::Rc;
use std::time::Duration;
use syncopate::{Drift, Scheduler, SimClock, TaskBuilder, Window};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn calculate_next_tick_single_task() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .name("every_500ms")
        .build();
    scheduler.add_task(task);

    assert_eq!(scheduler.calculate_next_tick(), Some(Duration::ZERO));

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn calculate_next_tick_returns_shortest_period() {
    let (_clock, mut scheduler) = make_scheduler();

    let fast = TaskBuilder::every(Duration::from_millis(100))
        .name("fast")
        .build();
    let slow = TaskBuilder::every(Duration::from_millis(500))
        .name("slow")
        .build();
    scheduler.add_task(fast);
    scheduler.add_task(slow);

    assert_eq!(scheduler.calculate_next_tick(), Some(Duration::ZERO));

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 2);

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

    let task = TaskBuilder::every(Duration::from_millis(500))
        .name("every_500ms")
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(499));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    clock.advance(Duration::from_millis(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
}

#[test]
fn task_fires_when_past_deadline_within_late_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("every_500ms")
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

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

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::from_millis(100), Duration::ZERO))
        .name("every_500ms")
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

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

    let task = TaskBuilder::every(Duration::from_millis(500))
        .name("every_500ms")
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

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

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::from_millis(100), Duration::ZERO))
        .name("every_500ms")
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(350));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);
}

#[test]
fn multiple_tasks_all_fire_at_shared_deadline() {
    let (clock, mut scheduler) = make_scheduler();

    for name in ["a", "b", "c"] {
        let task = TaskBuilder::every(Duration::from_millis(500))
            .name(name)
            .build();
        scheduler.add_task(task);
    }

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 3);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 3);
}
