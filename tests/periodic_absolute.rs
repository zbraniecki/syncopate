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
fn fires_at_wall_clock_boundary() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::symmetric(Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Before deadline: nothing fires.
    clock.advance(Duration::from_millis(399));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);

    // Exactly at 500ms: fires on time.
    clock.advance(Duration::from_millis(101));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn fires_within_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::new(Duration::from_millis(100), Duration::ZERO),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // 490ms = 10ms before the 500ms deadline, within the 100ms early window.
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

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // 510ms = 10ms after the 500ms deadline, within the 100ms late window.
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

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // 650ms = 150ms past the 500ms deadline, beyond the 100ms late window.
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

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::symmetric(Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Fire again at 1000ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn offset_tasks() {
    let (clock, mut scheduler) = make_scheduler();

    // period=1s, offset=200ms → fires at 200ms, 1200ms, ...
    let task = TaskBuilder::every_with_offset(
        Duration::from_secs(1),
        Duration::from_millis(200),
        Window::symmetric(Duration::from_millis(50)),
    )
    .name("every_1s_offset_200ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // At 100ms: before first deadline (200ms), nothing fires.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    // At 200ms: fires on time.
    clock.advance(Duration::from_millis(100));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // At 1000ms: not a deadline (deadlines are at 200, 1200, 2200, ...).
    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);

    // At 1200ms: fires on time.
    clock.advance(Duration::from_millis(200));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn wall_clock_jump_forward() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::symmetric(Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Jump wall clock forward by 2s (simulates suspend/resume).
    // Mono is at 500ms, wall is now at 2500ms.
    clock.jump_wall_clock(2_000_000_000);

    // Next tick should fire for the current wall-clock boundary (2500ms).
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn calculate_next_tick_returns_correct_duration() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_at_boundary(Duration::from_millis(500), Window::ZERO)
        .name("every_500ms")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // At time 0, the 0ms boundary is already "serviced" (initialized in add_task).
    // Next deadline is 500ms. With ZERO window, should return 500ms.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );

    // Advance to 300ms. Next deadline is still 500ms, so should return 200ms.
    clock.advance(Duration::from_millis(300));
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(200))
    );

    // Advance to 500ms and fire.
    clock.advance(Duration::from_millis(200));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // After firing at 500ms, next deadline is 1000ms. Should return 500ms.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn calculate_next_tick_with_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::new(Duration::from_millis(50), Duration::ZERO),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // At time 0, next deadline is 500ms. Should return 500ms (targets deadline,
    // not early window — the window is tolerance, not a scheduling target).
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );

    // Advance to 300ms. Deadline at 500ms, so 200ms to go.
    clock.advance(Duration::from_millis(300));
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(200))
    );
}

#[test]
fn does_not_fire_before_early_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every_at_boundary(
        Duration::from_millis(500),
        Window::new(Duration::from_millis(50), Duration::ZERO),
    )
    .name("every_500ms")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // At 440ms: 60ms before deadline, outside the 50ms early window.
    clock.advance(Duration::from_millis(440));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 0);
}
