use std::rc::Rc;
use std::time::Duration;
use syncopate::{Drift, Scheduler, SimClock, TaskBuilder, Window};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn fires_at_wall_clock_boundary() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::symmetric(Duration::from_millis(100)))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(399));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);

    clock.advance(Duration::from_millis(101));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn fires_within_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::new(Duration::from_millis(100), Duration::ZERO))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(490));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Early(Duration::from_millis(10))
    );
}

#[test]
fn fires_within_late_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(510));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(10))
    );
}

#[test]
fn missed_past_late_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(650));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(
        result.missed[0].deadlines_missed,
        vec![Duration::from_millis(150)]
    );
}

#[test]
fn consecutive_ticks() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::symmetric(Duration::from_millis(100)))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn offset_tasks() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_secs(1))
        .offset(Duration::from_millis(200))
        .window(Window::symmetric(Duration::from_millis(50)))
        .name("every_1s_offset_200ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    clock.advance(Duration::from_millis(200));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn wall_clock_jump_forward() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::symmetric(Duration::from_millis(100)))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.jump_wall_clock(2_000_000_000);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn calculate_next_tick_returns_correct_duration() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    assert_eq!(scheduler.calculate_next_tick(), Some(Duration::ZERO));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );

    clock.advance(Duration::from_millis(300));
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(200))
    );

    clock.advance(Duration::from_millis(200));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn calculate_next_tick_with_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::new(Duration::from_millis(50), Duration::ZERO))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    assert_eq!(scheduler.calculate_next_tick(), Some(Duration::ZERO));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );

    clock.advance(Duration::from_millis(300));
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(200))
    );
}

#[test]
fn does_not_fire_before_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_absolute(Duration::from_millis(500))
        .window(Window::new(Duration::from_millis(50), Duration::ZERO))
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(440));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);
}
