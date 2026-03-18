use std::rc::Rc;
use std::time::Duration;
use syncopate::{MissedTickBehavior, Scheduler, SimClock, TaskBuilder, Window};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn calculate_next_tick_respects_min_tick_interval() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100))
        .name("fast")
        .build();
    scheduler.add_task(task);

    scheduler.tick();

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(100))
    );

    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn min_tick_interval_causes_missed_deadlines() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100))
        .window(Window::symmetric(Duration::from_millis(10)))
        .name("fast")
        .build();
    scheduler.add_task(task);
    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();

    assert_eq!(
        result.fired.len(),
        1,
        "500ms deadline should fire (it's within window)"
    );
    assert!(
        !result.missed.is_empty(),
        "should report missed deadlines for 100-400ms"
    );
    assert_eq!(
        result.missed[0].deadlines_missed.len(),
        4,
        "4 deadlines missed (100-400ms)"
    );
}

#[test]
fn min_tick_interval_execute_mode_fires_latest() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100))
        .window(Window::symmetric(Duration::from_millis(10)))
        .name("fast")
        .on_miss(MissedTickBehavior::RunLatest)
        .build();
    scheduler.add_task(task);
    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    scheduler.tick();

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();

    assert_eq!(result.fired.len(), 1, "should fire for the latest deadline");
    assert!(
        !result.missed.is_empty(),
        "should report earlier missed deadlines"
    );
    assert_eq!(
        result.missed[0].deadlines_missed.len(),
        4,
        "4 deadlines missed (100-400ms)"
    );
}

#[test]
fn wide_window_task_survives_min_tick_interval() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(200))
        .window(Window::new(Duration::ZERO, Duration::from_millis(200)))
        .name("wide")
        .build();
    scheduler.add_task(task);
    scheduler.set_min_tick_interval(Some(Duration::from_millis(300)));

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(300));
    let result = scheduler.tick();

    assert_eq!(
        result.fired.len(),
        1,
        "wide window task should still fire (late but in window)"
    );
    assert!(result.missed.is_empty(), "should not have missed deadlines");
}

#[test]
fn min_tick_interval_none_has_no_effect() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100))
        .name("fast")
        .build();
    scheduler.add_task(task);

    scheduler.tick();

    scheduler.set_min_tick_interval(None);

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(100))
    );
}

#[test]
fn min_tick_interval_does_not_reduce_natural_sleep() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_secs(1))
        .name("slow")
        .build();
    scheduler.add_task(task);

    scheduler.tick();

    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_secs(1))
    );
}

#[test]
fn period_500ms_fires_every_1s_with_min_tick_interval() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .name("half-second")
        .build();
    scheduler.add_task(task);
    scheduler.set_min_tick_interval(Some(Duration::from_secs(1)));

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_secs(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "1s deadline should fire");
    assert_eq!(
        result.missed[0].deadlines_missed.len(),
        1,
        "500ms deadline missed"
    );

    clock.advance(Duration::from_secs(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "2s deadline should fire");
    assert_eq!(
        result.missed[0].deadlines_missed.len(),
        1,
        "1500ms deadline missed"
    );

    clock.advance(Duration::from_secs(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "3s deadline should fire");
    assert_eq!(
        result.missed[0].deadlines_missed.len(),
        1,
        "2500ms deadline missed"
    );
}
