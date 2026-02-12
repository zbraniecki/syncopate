use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

const NANOS_PER_MS: u64 = 1_000_000;
const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Test case for clock-anchored periodic tasks.
/// These tasks fire at absolute clock boundaries (e.g., every whole second).
struct AnchoredTestCase {
    /// Period for the anchored task (in nanoseconds)
    period_ns: u64,
    /// Current time within the period when task is added (in nanoseconds)
    /// For example, if period is 1s and we're at 300ms, this would be 300_000_000
    current_offset_ns: u64,
    /// Expected duration until first tick (should align to next period boundary)
    expected_first_tick_ns: u64,
    /// Expected duration for subsequent ticks (should equal period)
    expected_subsequent_tick_ns: u64,
}

#[test]
fn test_single_anchored_task() {
    let test_cases: &[AnchoredTestCase] = &[
        // Added exactly at boundary (t=0s) - first tick at 1s
        AnchoredTestCase {
            period_ns: 1 * NANOS_PER_SEC,
            current_offset_ns: 0,
            expected_first_tick_ns: 1 * NANOS_PER_SEC,
            expected_subsequent_tick_ns: 1 * NANOS_PER_SEC,
        },
        // Added at 300ms - first tick at 1s (700ms wait)
        AnchoredTestCase {
            period_ns: 1 * NANOS_PER_SEC,
            current_offset_ns: 300 * NANOS_PER_MS,
            expected_first_tick_ns: 700 * NANOS_PER_MS,
            expected_subsequent_tick_ns: 1 * NANOS_PER_SEC,
        },
        // Added at 999ms - first tick at 1s (1ms wait)
        AnchoredTestCase {
            period_ns: 1 * NANOS_PER_SEC,
            current_offset_ns: 999 * NANOS_PER_MS,
            expected_first_tick_ns: 1 * NANOS_PER_MS,
            expected_subsequent_tick_ns: 1 * NANOS_PER_SEC,
        },
    ];

    for case in test_cases {
        // Create scheduler with epoch at t=0, current time at the offset
        let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let current_time = epoch + Duration::from_nanos(case.current_offset_ns);
        let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, current_time);

        let task = TaskBuilder::every_at_boundary(Duration::from_nanos(case.period_ns))
            .name("anchored")
            .build()
            .unwrap();

        scheduler.add_task(task).unwrap();

        // Check first tick
        let next_tick = scheduler.calculate_next_tick();
        assert_eq!(
            next_tick,
            Some(Duration::from_nanos(case.expected_first_tick_ns)),
            "First tick: offset {}ns in {}ns period should wait {}ns, got {:?}",
            case.current_offset_ns,
            case.period_ns,
            case.expected_first_tick_ns,
            next_tick
        );

        // Execute first tick
        let ready = scheduler.tick(Duration::from_nanos(case.expected_first_tick_ns));
        assert_eq!(ready.len(), 1);

        // Check subsequent tick
        let next_tick = scheduler.calculate_next_tick();
        assert_eq!(
            next_tick,
            Some(Duration::from_nanos(case.expected_subsequent_tick_ns)),
            "Subsequent tick should be {}ns",
            case.expected_subsequent_tick_ns
        );
    }
}

#[test]
fn test_two_anchored_tasks() {
    // Scenario: Clock starts at 200ms into the current second
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler =
        Scheduler::with_test_time(epoch, epoch + Duration::from_nanos(200 * NANOS_PER_MS));

    // Add first task at scheduler time 0 (clock at 200ms)
    let task0 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("0")
        .build()
        .unwrap();
    scheduler.add_task(task0).unwrap();

    // Advance 300ms (clock now at 500ms into the second)
    scheduler.advance_time(epoch + Duration::from_nanos(500 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0); // No tasks fire yet

    // Add second task at scheduler time 300ms (clock at 500ms)
    let task1 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("1")
        .build()
        .unwrap();
    scheduler.add_task(task1).unwrap();

    // Both tasks should now fire at the 1-second boundary
    // Task 0: added at epoch 200ms, fires at 1000ms (800ms from scheduler start)
    // Task 1: added at epoch 500ms, fires at 1000ms (800ms from scheduler start, 500ms from add)
    // Next tick should be 500ms away (to reach 800ms total)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(500 * NANOS_PER_MS)));

    // Execute - both tasks fire together at the 1-second boundary
    scheduler.advance_time(epoch + Duration::from_nanos(1000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(500 * NANOS_PER_MS));
    assert_eq!(ready.len(), 2);

    // Now both are synchronized - next tick at 1s, both fire
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(1 * NANOS_PER_SEC)));

    scheduler.advance_time(epoch + Duration::from_nanos(2000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
    assert_eq!(ready.len(), 2);
}

#[test]
fn test_three_anchored_tasks() {
    // Scenario: Add three tasks at different times as clock advances
    // Clock starts at 100ms into the current second
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler =
        Scheduler::with_test_time(epoch, epoch + Duration::from_nanos(100 * NANOS_PER_MS));

    // Task 0: added at scheduler time 0ms (clock at 100ms)
    let task0 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("0")
        .build()
        .unwrap();
    scheduler.add_task(task0).unwrap();

    // Advance 300ms (clock now at 400ms)
    scheduler.advance_time(epoch + Duration::from_nanos(400 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0);

    // Task 1: added at scheduler time 300ms (clock at 400ms)
    let task1 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("1")
        .build()
        .unwrap();
    scheduler.add_task(task1).unwrap();

    // Advance 300ms more (clock now at 700ms)
    scheduler.advance_time(epoch + Duration::from_nanos(700 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0);

    // Task 2: added at scheduler time 600ms (clock at 700ms)
    let task2 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("2")
        .build()
        .unwrap();
    scheduler.add_task(task2).unwrap();

    // All three tasks should fire at the next 1-second boundary
    // Task 0: added at epoch 100ms, next boundary at 1000ms (900ms from start)
    // Task 1: added at epoch 400ms, next boundary at 1000ms (900ms from start)
    // Task 2: added at epoch 700ms, next boundary at 1000ms (900ms from start)
    // Currently at 600ms, so 300ms until all fire
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(300 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(1000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 3);

    // Verify all subsequent ticks are at 1s intervals with all three firing
    for i in 0..3 {
        let next_tick = scheduler.calculate_next_tick();
        assert_eq!(next_tick, Some(Duration::from_nanos(1 * NANOS_PER_SEC)));
        scheduler.advance_time(epoch + Duration::from_nanos((2000 + i * 1000) * NANOS_PER_MS));
        let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
        assert_eq!(ready.len(), 3);
    }
}
