use std::rc::Rc;
use std::time::Duration;
use syncopate::{
    MissedTickBehavior, Window,
    scheduler::Scheduler,
    system_time::SimClock,
    task::TaskBuilder,
};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn calculate_next_tick_respects_min_tick_interval() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("fast")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume immediate fire at t=0.
    scheduler.tick();

    // Without min_tick_interval, next tick should be at 100ms.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(100))
    );

    // Set min_tick_interval to 500ms.
    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    // Now calculate_next_tick should return 500ms (max of 100ms and 500ms).
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn min_tick_interval_causes_missed_deadlines() {
    let (clock, mut scheduler) = make_scheduler();

    // 100ms period, tight 10ms window, skip missed ticks.
    let task = TaskBuilder::every(
        Duration::from_millis(100),
        Window::symmetric(Duration::from_millis(10)),
    )
    .name("fast")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();
    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    // Consume immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Advance 500ms (as min_tick_interval demands).
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();

    // The 500ms deadline is exactly at `now`, which is within its ±10ms window.
    // So it fires, while 100-400ms are missed (4 missed deadlines).
    assert_eq!(result.fired.len(), 1, "500ms deadline should fire (it's within window)");
    assert!(!result.missed.is_empty(), "should report missed deadlines for 100-400ms");
    assert_eq!(result.missed[0].deadlines_missed.len(), 4, "4 deadlines missed (100-400ms)");
}

#[test]
fn min_tick_interval_execute_mode_fires_latest() {
    let (clock, mut scheduler) = make_scheduler();

    // 100ms period, tight 10ms window, execute on miss.
    let task = TaskBuilder::every(
        Duration::from_millis(100),
        Window::symmetric(Duration::from_millis(10)),
    )
    .name("fast")
    .on_miss(MissedTickBehavior::Execute)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();
    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    // Consume immediate fire at t=0.
    scheduler.tick();

    // Advance 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();

    // The 500ms deadline is within its window, so it fires directly (not via Execute miss path).
    assert_eq!(result.fired.len(), 1, "should fire for the latest deadline");
    assert!(!result.missed.is_empty(), "should report earlier missed deadlines");
    assert_eq!(result.missed[0].deadlines_missed.len(), 4, "4 deadlines missed (100-400ms)");
}

#[test]
fn wide_window_task_survives_min_tick_interval() {
    let (clock, mut scheduler) = make_scheduler();

    // 200ms period with 200ms late window — deadline at 200ms is valid until 400ms.
    let task = TaskBuilder::every(
        Duration::from_millis(200),
        Window::new(Duration::ZERO, Duration::from_millis(200)),
    )
    .name("wide")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();
    scheduler.set_min_tick_interval(Some(Duration::from_millis(300)));

    // Consume immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Advance 300ms (as min_tick_interval demands).
    // Deadline was at 200ms, window extends to 400ms, so 300ms is within window.
    clock.advance(Duration::from_millis(300));
    let result = scheduler.tick();

    assert_eq!(result.fired.len(), 1, "wide window task should still fire (late but in window)");
    assert!(result.missed.is_empty(), "should not have missed deadlines");
}

#[test]
fn min_tick_interval_none_has_no_effect() {
    let (_clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(100), Window::ZERO)
        .name("fast")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume immediate fire at t=0.
    scheduler.tick();

    // Set to None explicitly.
    scheduler.set_min_tick_interval(None);

    // Should return the natural 100ms.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_millis(100))
    );
}

#[test]
fn min_tick_interval_does_not_reduce_natural_sleep() {
    let (_clock, mut scheduler) = make_scheduler();

    // Task with 1s period — natural sleep is already larger than min_tick_interval.
    let task = TaskBuilder::every(Duration::from_secs(1), Window::ZERO)
        .name("slow")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume immediate fire at t=0.
    scheduler.tick();

    scheduler.set_min_tick_interval(Some(Duration::from_millis(500)));

    // Natural sleep is 1s, min is 500ms — should still return 1s.
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_secs(1))
    );
}

#[test]
fn period_500ms_fires_every_1s_with_min_tick_interval() {
    let (clock, mut scheduler) = make_scheduler();

    // 500ms period, zero window, skip missed ticks, min_tick_interval=1s.
    // Every 1s tick should fire the 1s/2s/3s deadlines (they coincide with `now`),
    // while 500ms/1500ms/2500ms deadlines are missed.
    let task = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
        .name("half-second")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();
    scheduler.set_min_tick_interval(Some(Duration::from_secs(1)));

    // Consume immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Tick at t=1s: deadline at 1000ms is exactly at `now` (within ZERO window),
    // deadline at 500ms is missed.
    clock.advance(Duration::from_secs(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "1s deadline should fire");
    assert_eq!(result.missed[0].deadlines_missed.len(), 1, "500ms deadline missed");

    // Tick at t=2s: deadline at 2000ms fires, 1500ms missed.
    clock.advance(Duration::from_secs(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "2s deadline should fire");
    assert_eq!(result.missed[0].deadlines_missed.len(), 1, "1500ms deadline missed");

    // Tick at t=3s: deadline at 3000ms fires, 2500ms missed.
    clock.advance(Duration::from_secs(1));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1, "3s deadline should fire");
    assert_eq!(result.missed[0].deadlines_missed.len(), 1, "2500ms deadline missed");
}
