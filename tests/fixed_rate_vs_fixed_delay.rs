use std::rc::Rc;
use std::time::Duration;
use syncopate::{
    Drift, MissedTickBehavior, PeriodicSchedule, Scheduler, SimClock, TaskBuilder, Window,
};

fn make_scheduler() -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    (clock, scheduler)
}

#[test]
fn sync_work_after_on_time_tick_compensated_equally() {
    for schedule in [PeriodicSchedule::FixedRate, PeriodicSchedule::FixedDelay] {
        let (clock, mut scheduler) = make_scheduler();

        let task = TaskBuilder::every(Duration::from_millis(500))
            .name("task")
            .schedule(schedule)
            .build();
        scheduler.add_task(task);

        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "{schedule:?}: immediate fire");
        assert_eq!(
            result.fired[0].drift,
            Drift::OnTime,
            "{schedule:?}: on time at t=0"
        );

        clock.advance(Duration::from_millis(500));
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "{schedule:?}: should fire");
        assert_eq!(
            result.fired[0].drift,
            Drift::OnTime,
            "{schedule:?}: on time"
        );

        clock.advance(Duration::from_millis(30));

        let next = scheduler.calculate_next_tick().unwrap();
        assert_eq!(next, Duration::from_millis(470), "{schedule:?}: next tick");
    }
}

#[test]
fn fixed_rate_catches_up_after_late_fire() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(20))
    );

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(480));

    clock.advance(Duration::from_millis(480));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn fixed_delay_does_not_catch_up_after_late_fire() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedDelay)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(20))
    );

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn fixed_rate_late_fire_plus_sync_work() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(30));

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(450));

    clock.advance(Duration::from_millis(450));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn fixed_delay_late_fire_plus_sync_work() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedDelay)
        .build();
    scheduler.add_task(task);

    clock.advance(Duration::from_millis(520));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(30));

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(470));

    clock.advance(Duration::from_millis(470));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn multi_cycle_drift_accumulation() {
    {
        let (clock, mut scheduler) = make_scheduler();

        let task = TaskBuilder::every(Duration::from_millis(500))
            .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
            .name("task")
            .schedule(PeriodicSchedule::FixedRate)
            .build();
        scheduler.add_task(task);

        for cycle in 0..5 {
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

    {
        let (clock, mut scheduler) = make_scheduler();

        let task = TaskBuilder::every(Duration::from_millis(500))
            .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
            .name("task")
            .schedule(PeriodicSchedule::FixedDelay)
            .build();
        scheduler.add_task(task);

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
        }
    }
}

#[test]
fn fixed_rate_realigns_to_grid_after_miss() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 0);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(
        result.missed[0].deadlines_missed,
        vec![Duration::from_millis(300)]
    );

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(200));
}

#[test]
fn fixed_delay_always_fires_past_window() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedDelay)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.missed.len(), 0);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(300))
    );

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));
}

#[test]
fn fixed_rate_large_miss_skips_many_periods() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(5000));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 9);

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}

#[test]
fn fixed_rate_miss_then_recovery_preserves_grid_long_term() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build();
    scheduler.add_task(task);

    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.missed.len(), 1);

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(200));
    clock.advance(next);
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    for cycle in 0..5 {
        let next = scheduler.calculate_next_tick().unwrap();
        assert_eq!(next, Duration::from_millis(500), "cycle {cycle}");
        clock.advance(next);
        let result = scheduler.tick();
        assert_eq!(result.fired.len(), 1, "cycle {cycle}");
        assert_eq!(result.fired[0].drift, Drift::OnTime, "cycle {cycle}");
    }
}

#[test]
fn execute_single_period_late_fires() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .on_miss(MissedTickBehavior::RunLatest)
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(800));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.missed.len(), 0);
    assert_eq!(
        result.fired[0].drift,
        Drift::Late(Duration::from_millis(300))
    );

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(200));
}

#[test]
fn execute_multi_period_late() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .on_miss(MissedTickBehavior::RunLatest)
        .build();
    scheduler.add_task(task);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(3000));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 5);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));
}

#[test]
fn burst_unlimited_fires_all() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .on_miss(MissedTickBehavior::Burst { max: None })
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(2500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 4);

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));
}

#[test]
fn burst_capped_fires_with_overflow() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .on_miss(MissedTickBehavior::Burst { max: Some(3) })
        .build();
    scheduler.add_task(task);

    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);

    clock.advance(Duration::from_millis(2500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 4);
}

#[test]
fn fixed_rate_miss_at_exact_grid_boundary() {
    let (clock, mut scheduler) = make_scheduler();

    let task = TaskBuilder::every(Duration::from_millis(500))
        .window(Window::new(Duration::ZERO, Duration::from_millis(100)))
        .name("task")
        .schedule(PeriodicSchedule::FixedRate)
        .build();
    scheduler.add_task(task);

    clock.advance(Duration::from_millis(1000));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
    assert_eq!(result.missed.len(), 1);
    assert_eq!(result.missed[0].deadlines_missed.len(), 2);

    let next = scheduler.calculate_next_tick().unwrap();
    assert_eq!(next, Duration::from_millis(500));

    clock.advance(Duration::from_millis(500));
    let result = scheduler.tick();
    assert_eq!(result.fired.len(), 1);
    assert_eq!(result.fired[0].drift, Drift::OnTime);
}
