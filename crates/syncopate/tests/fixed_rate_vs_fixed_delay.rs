use std::rc::Rc;
use std::time::Duration;
use syncopate::{
    PeriodicSchedule, Window,
    scheduler::{Drift, Scheduler},
    system_time::SimClock,
    task::TaskBuilder,
};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

/// Both FixedRate and FixedDelay shorten the next sleep to compensate for
/// post-tick sync work when the tick fires exactly on time.
#[test]
fn sync_work_after_on_time_tick_compensated_equally() {
    for schedule in [PeriodicSchedule::FixedRate, PeriodicSchedule::FixedDelay] {
        let (clock, mut scheduler) = make_scheduler();

        let task = TaskBuilder::every(Duration::from_millis(500), Window::ZERO)
            .name("task")
            .schedule(schedule)
            .build()
            .unwrap();
        scheduler.add_task(task).unwrap();

        // Advance to exactly the deadline and fire.
        clock.advance(Duration::from_millis(500));
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "{schedule:?}: should fire");
        assert_eq!(result.fired[0].drift, Drift::OnTime, "{schedule:?}: on time");

        // Simulate 30ms of sync work after tick.
        clock.advance(Duration::from_millis(30));

        // Next sleep should be 500 - 30 = 470ms (both modes identical here).
        let next = scheduler.calculate_next_tick().unwrap();
        assert_eq!(next, Duration::from_millis(470), "{schedule:?}: next tick");
    }
}

/// FixedRate: after a late fire, the next sleep is shortened to catch up
/// to the absolute schedule.
#[test]
fn fixed_rate_catches_up_after_late_fire() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedRate)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire 20ms late (at 520ms, within the 100ms late window).
    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::Late(Duration::from_millis(20)));

    // FixedRate anchored last_fired to ideal deadline (500ms).
    // Next deadline = 1000ms. Now = 520ms. Sleep = 1000 - 520 = 480ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(480));

    // Sleep and fire — should land exactly on the ideal deadline.
    clock.advance(Duration::from_millis(480));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

/// FixedDelay: after a late fire, the next period starts from the actual
/// fire time — no catch-up, cadence shifts.
#[test]
fn fixed_delay_does_not_catch_up_after_late_fire() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedDelay)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire 20ms late (at 520ms).
    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::Late(Duration::from_millis(20)));

    // FixedDelay anchored last_fired to now (520ms).
    // Next deadline = 520 + 500 = 1020ms. Now = 520ms. Sleep = 500ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));

    // Sleep and fire — lands at 1020ms, on the shifted deadline.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

/// FixedRate: late fire + sync work still catches up to the absolute schedule.
#[test]
fn fixed_rate_late_fire_plus_sync_work() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedRate)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire 20ms late.
    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // 30ms of sync work. Now = 550ms.
    clock.advance(Duration::from_millis(30));

    // Next deadline = 1000ms. Sleep = 1000 - 550 = 450ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(450));

    // Sleep and fire — back on the ideal schedule at 1000ms.
    clock.advance(Duration::from_millis(450));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

/// FixedDelay: late fire + sync work — period starts from fire time,
/// sync work is compensated but the late shift persists.
#[test]
fn fixed_delay_late_fire_plus_sync_work() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedDelay)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire 20ms late.
    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // 30ms of sync work. Now = 550ms.
    clock.advance(Duration::from_millis(30));

    // Next deadline = 520 + 500 = 1020ms. Sleep = 1020 - 550 = 470ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(470));

    // Sleep and fire — lands at 1020ms (shifted by the original 20ms).
    clock.advance(Duration::from_millis(470));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

/// Over multiple cycles, FixedRate maintains absolute cadence while
/// FixedDelay accumulates drift from late fires.
#[test]
fn multi_cycle_drift_accumulation() {
    // FixedRate: fire 20ms late each cycle, but always catches up.
    {
        let (clock, mut scheduler) = make_scheduler();

        let task = TaskBuilder::every(
            Duration::from_millis(500),
            Window::new(Duration::ZERO, Duration::from_millis(100)),
        )
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build()
        .unwrap();
        scheduler.add_task(task).unwrap();

        for cycle in 0..5 {
            // Always fire 20ms late.
            let sleep = scheduler.calculate_next_tick().unwrap();
            clock.advance(sleep + Duration::from_millis(20));

            let result = scheduler.tick();
            assert_eq!(result.fired.len(), 1, "FixedRate cycle {cycle}");
            assert_eq!(
                result.fired[0].drift,
                Drift::Late(Duration::from_millis(20)),
                "FixedRate cycle {cycle}: drift should be +20ms"
            );
        }
    }

    // FixedDelay: fire 20ms late each cycle — drift is always +20ms
    // relative to the shifted deadline, but absolute position shifts
    // by 20ms per cycle.
    {
        let (clock, mut scheduler) = make_scheduler();

        let task = TaskBuilder::every(
            Duration::from_millis(500),
            Window::new(Duration::ZERO, Duration::from_millis(100)),
        )
        .name("task")
        .schedule(PeriodicSchedule::FixedDelay)
        .build()
        .unwrap();
        scheduler.add_task(task).unwrap();

        for cycle in 0..5 {
            let sleep = scheduler.calculate_next_tick().unwrap();
            clock.advance(sleep + Duration::from_millis(20));

            let result = scheduler.tick();
            assert_eq!(result.fired.len(), 1, "FixedDelay cycle {cycle}");
            assert_eq!(
                result.fired[0].drift,
                Drift::Late(Duration::from_millis(20)),
                "FixedDelay cycle {cycle}: drift should be +20ms"
            );

            // FixedDelay always returns 500ms because it anchors to now.
            // (We haven't done any sync work, so now == fire time.)
        }

        // After 5 cycles with +20ms oversleep each, FixedDelay's absolute
        // position is at 5*520 = 2600ms, not 5*500 = 2500ms.
        // FixedRate's position would still be at 2500ms + 20ms = 2520ms
        // (only the current cycle's overshoot, prior cycles corrected).
    }
}
