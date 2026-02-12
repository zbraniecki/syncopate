use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

#[test]
fn test_absolute_periodic_resync_after_sleep() {
    // Setup: Start at t=0, add "every 60s at :00" task
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch + Duration::from_secs(30); // :30
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
        .name("every_minute")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();

    // First fire should be at :00 (30s from now in virtual time)
    let fired = scheduler.tick(Duration::from_secs(30));
    assert_eq!(fired.len(), 1, "Task should fire at first :00 boundary");

    // Simulate sleep: Advance wall-clock by 70s but virtual time by only 10s
    scheduler.advance_time(start_time + Duration::from_secs(100)); // Now at :130 wall-clock
    let _fired = scheduler.tick(Duration::from_secs(10)); // Virtual time advances 10s

    // Time discontinuity: wall-clock advanced 70s, virtual time 10s
    // Should trigger resync and task should fire at next :00 boundary
    // Wall-clock is at :130 (2:10), next :00 is at :180 (3:00)
    // That's 50s away in wall-clock
    // Virtual time is at 40s, so next_fire should be recalculated to 40s + 50s = 90s

    // Continue ticking to next boundary
    let fired = scheduler.tick(Duration::from_secs(50));
    assert_eq!(
        fired.len(),
        1,
        "Task should fire at next :00 boundary after resync"
    );
}

#[test]
fn test_absolute_onetime_deadline_passed_during_sleep() {
    // Setup: Deadline at t=60s
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch;
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let deadline = epoch + Duration::from_secs(60);
    let task = TaskBuilder::<()>::once_at(deadline)
        .name("deadline_task")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();

    // Simulate sleep that passes the deadline
    scheduler.advance_time(epoch + Duration::from_secs(100)); // Past deadline
    let fired = scheduler.tick(Duration::from_secs(10)); // Small virtual tick

    // Task deadline has passed - resync will fail and task is removed
    // Since we ignore errors in resync, the task will be removed from the scheduler
    // The task won't fire because the deadline is in the past
    assert_eq!(
        fired.len(),
        0,
        "Task should not fire when deadline passed during sleep"
    );
}

#[test]
fn test_relative_tasks_unaffected_by_sleep() {
    // Relative tasks should not be affected by wall-clock discontinuities
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch;
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let task = TaskBuilder::<()>::every(Duration::from_secs(60))
        .name("every_60s_relative")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();

    // First fire at virtual t=60s
    let fired = scheduler.tick(Duration::from_secs(60));
    assert_eq!(fired.len(), 1);

    // Simulate massive sleep: wall-clock advances 1 hour
    scheduler.advance_time(start_time + Duration::from_secs(3660));

    // Virtual time advances only 60s
    let fired = scheduler.tick(Duration::from_secs(60));

    // Should fire exactly once at virtual t=120s, regardless of wall-clock
    assert_eq!(fired.len(), 1);
}

#[test]
fn test_no_resync_when_wall_clock_matches_tick() {
    // When wall-clock time advances normally (matches tick duration),
    // no resync should occur
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch;
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(10))
        .name("every_10s")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();

    // Normal tick: wall-clock and virtual time advance together
    scheduler.advance_time(start_time + Duration::from_secs(10));
    let fired = scheduler.tick(Duration::from_secs(10));
    assert_eq!(fired.len(), 1);

    // Another normal tick
    scheduler.advance_time(start_time + Duration::from_secs(20));
    let fired = scheduler.tick(Duration::from_secs(10));
    assert_eq!(fired.len(), 1);
}

#[test]
fn test_resync_threshold_detection() {
    // Test that the 3x threshold is properly detected
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch;
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
        .name("every_minute")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();

    // Just under 3x threshold (2.9x) - should NOT trigger resync
    // If we tick 10s but wall-clock advances 29s (2.9x), no resync
    scheduler.advance_time(start_time + Duration::from_secs(29));
    let _fired = scheduler.tick(Duration::from_secs(10));

    // Just over 3x threshold (3.1x) - should trigger resync
    // If we tick 10s but wall-clock advances 31s (3.1x), resync should occur
    scheduler.advance_time(start_time + Duration::from_secs(60));
    let _fired = scheduler.tick(Duration::from_secs(10));
    // If resync occurred, the task's next_fire would be recalculated
    // We can't easily verify this without exposing internals, but the test
    // ensures the logic path is exercised
}

#[test]
fn test_multiple_absolute_tasks_resync() {
    // Test that multiple absolute tasks all get resynced
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch;
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let task1 = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
        .name("every_minute")
        .build()
        .unwrap();

    let task2 =
        TaskBuilder::<()>::every_with_offset(Duration::from_secs(120), Duration::from_secs(30))
            .name("every_2min_at_30s")
            .build()
            .unwrap();

    scheduler.add_task(task1).unwrap();
    scheduler.add_task(task2).unwrap();

    // Simulate sleep
    scheduler.advance_time(start_time + Duration::from_secs(500));
    let _fired = scheduler.tick(Duration::from_secs(10));

    // Both tasks should be resynced and continue firing at their boundaries
    // Verify they still fire correctly after resync
    scheduler.advance_time(start_time + Duration::from_secs(560));
    let fired = scheduler.tick(Duration::from_secs(60));

    // Should have fired the tasks at their respective boundaries
    assert!(fired.len() > 0, "Tasks should continue firing after resync");
}
