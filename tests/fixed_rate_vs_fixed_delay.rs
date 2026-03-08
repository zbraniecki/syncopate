use std::rc::Rc;
use std::time::Duration;
use syncopate::{
    MissedTickBehavior, PeriodicSchedule, Window,
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

        // Consume the immediate fire at t=0.
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "{schedule:?}: immediate fire");
        assert_eq!(
            result.fired[0].drift,
            Drift::OnTime,
            "{schedule:?}: on time at t=0"
        );

        // Advance to exactly the deadline and fire.
        clock.advance(Duration::from_millis(500));
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "{schedule:?}: should fire");
        assert_eq!(
            result.fired[0].drift,
            Drift::OnTime,
            "{schedule:?}: on time"
        );

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

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Fire 20ms late (at 520ms, within the 100ms late window).
    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(20))
    );

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

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Fire 20ms late (at 520ms).
    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(20))
    );

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

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

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

/// FixedRate: after a miss (beyond window), re-aligns to the periodic grid.
/// period=500ms, window=100ms late, miss at 800ms.
/// Next deadline should be 1000ms (grid), not 1300ms (now + period).
#[test]
fn fixed_rate_realigns_to_grid_after_miss() {
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

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Miss at 800ms (deadline was 500ms, window ends at 600ms).
    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(
        result.missed[0].deadlines_missed,
        vec![Duration::from_millis(300)]
    );

    // FixedRate should re-align: last_fired = 500ms (grid point).
    // Next deadline = 500 + 500 = 1000ms. Sleep = 1000 - 800 = 200ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(200));
}

/// FixedDelay: always fires past window (window is informational only).
/// Next period starts from `now` — no grid alignment.
#[test]
fn fixed_delay_always_fires_past_window() {
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

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // 800ms — past the 100ms late window, but FixedDelay always fires.
    // Deadline is at 500ms (0 + period since FixedDelay anchored last_fired to t=0).
    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.missed.len(), 0);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(300))
    );

    // FixedDelay anchors to now: next deadline = 800 + 500 = 1300ms. Sleep = 500ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));
}

/// FixedRate: large miss (5 seconds) skips many periods but reports 1 miss,
/// then recovers back to grid.
#[test]
fn fixed_rate_large_miss_skips_many_periods() {
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

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Fire normally at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Simulate 5-second sleep. Now = 5500ms, deadline was 1000ms.
    // Missed deadlines: 1000, 1500, 2000, 2500, 3000, 3500, 4000, 4500, 5000, 5500.
    // Lateness:         4500, 4000, 3500, 3000, 2500, 2000, 1500, 1000,  500,    0.
    clock.advance(Duration::from_millis(5000));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 10);
    assert_eq!(
        result.missed[0].deadlines_missed[0],
        Duration::from_millis(4500)
    );
    assert_eq!(result.missed[0].deadlines_missed[9], Duration::ZERO);

    // Grid re-alignment: elapsed = 5500 - 1000 = 4500ms.
    // periods_elapsed = 4500 / 500 = 9. last_fired = 1000 + 500*9 = 5500ms.
    // Next deadline = 5500 + 500 = 6000ms. Sleep = 6000 - 5500 = 500ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));

    // Fire at 6000ms — back on grid.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

/// FixedRate: after miss recovery, subsequent ticks stay on the original grid.
#[test]
fn fixed_rate_miss_then_recovery_preserves_grid_long_term() {
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

    // Miss at 800ms.
    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.missed.len(), 1);

    // Recover: next tick at 1000ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(200));
    clock.advance(next);
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Verify 5 subsequent ticks stay on grid (1500, 2000, 2500, 3000, 3500).
    for cycle in 0..5 {
        let next = scheduler.calculate_next_tick().unwrap();
        assert_eq!(next, Duration::from_millis(500), "cycle {cycle}");
        clock.advance(next);
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "cycle {cycle}");
        assert_eq!(result.fired[0].drift, Drift::OnTime, "cycle {cycle}");
    }
}

// ── Execute mode tests ──────────────────────────────────────────────────────

/// Execute: single-period late → fires once (no MissedExecution).
#[test]
fn execute_single_period_late_fires() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedRate)
    .on_miss(MissedTickBehavior::Execute)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // 800ms — 300ms past deadline (500ms), 200ms past window end (600ms).
    // Only 1 deadline missed, so Execute fires for it directly.
    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.missed.len(), 0);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(300))
    );

    // Re-aligns to grid: last_fired = 500ms. Next deadline = 1000ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(200));
}

/// Execute: multi-period late → MissedExecution for earlier + TaskExecution for latest.
#[test]
fn execute_multi_period_late() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedRate)
    .on_miss(MissedTickBehavior::Execute)
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Fire normally at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Now at 3500ms (deadline was 1000ms).
    // elapsed = 3500 - 1000 = 2500ms. periods_elapsed = 2500/500 = 5. count = 6.
    // Deadlines: 1000, 1500, 2000, 2500, 3000, 3500 → 6 total.
    // Execute: skip first 5 (MissedExecution), fire for 3500 (latest).
    clock.advance(Duration::from_millis(3000));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 5);
    // Latest deadline = 3500ms = now. Drift = 0.
    assert_eq!(result.fired[0].drift, Drift::Late(Duration::ZERO));

    // Grid re-alignment: last_fired = 3500ms. Next = 4000ms. Sleep = 500ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));
}

// ── Burst mode tests ────────────────────────────────────────────────────────

/// Burst(None): fires once per missed period (rapid catch-up).
#[test]
fn burst_unlimited_fires_all() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedRate)
    .on_miss(MissedTickBehavior::Burst { max: None })
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Fire normally at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Now at 3000ms. Deadlines: 1000, 1500, 2000, 2500, 3000 → 5 total.
    // elapsed = 3000 - 1000 = 2000ms. periods_elapsed = 2000/500 = 4. count = 5.
    // Burst(None): fire all 5.
    clock.advance(Duration::from_millis(2500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 5);
    assert_eq!(result.missed.len(), 0);
    // Most recent fire has smallest drift.
    assert_eq!(result.fired[4].drift, Drift::Late(Duration::ZERO));
    // Earliest fire has largest drift.
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(2000))
    );

    // Grid re-alignment: last_fired = 3000ms. Next = 3500ms. Sleep = 500ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));
}

/// Burst(Some(3)): caps fires, overflow → MissedExecution.
#[test]
fn burst_capped_fires_with_overflow() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::new(Duration::ZERO, Duration::from_millis(100)),
    )
    .name("task")
    .schedule(PeriodicSchedule::FixedRate)
    .on_miss(MissedTickBehavior::Burst { max: Some(3) })
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // Consume the immediate fire at t=0.
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    // Fire normally at 500ms.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    // Now at 3000ms. Deadlines: 1000, 1500, 2000, 2500, 3000 → 5 total.
    // Burst(max=3): fire 3 most recent (2000, 2500, 3000), skip 2 (1000, 1500).
    clock.advance(Duration::from_millis(2500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 3);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 2);
    // Skipped deadlines have largest lateness (most late first).
    assert_eq!(
        result.missed[0].deadlines_missed[0],
        Duration::from_millis(2000)
    );
    assert_eq!(
        result.missed[0].deadlines_missed[1],
        Duration::from_millis(1500)
    );
}

/// Edge case: miss lands exactly on a grid point.
#[test]
fn fixed_rate_miss_at_exact_grid_boundary() {
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

    // Miss at exactly 1000ms (deadline was 500ms, window ends at 600ms).
    // This is exactly the next grid point.
    clock.advance(Duration::from_millis(1000));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);

    // elapsed = 1000 - 500 = 500ms. periods_elapsed = 500/500 = 1.
    // last_fired = 500 + 500*1 = 1000ms. Next deadline = 1500ms. Sleep = 500ms.
    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));

    // Fire at 1500ms — on grid.
    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}
