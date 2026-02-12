use std::time::Duration;
use syncopate::scheduler::Scheduler;
use syncopate::task::{Task, TaskType};

const NANOS_PER_MS: u64 = 1_000_000;
const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Test case for verifying initial next_tick calculation.
struct NextTickTestCase {
    /// Periods of tasks to add (in nanoseconds)
    task_periods_ns: &'static [u64],
    /// Expected duration until next tick (None if scheduler is empty)
    expected_next_tick_ns: Option<u64>,
}

#[test]
fn test_scheduler_next_tick() {
    let test_cases: &[NextTickTestCase] = &[
        // Empty scheduler returns None
        NextTickTestCase {
            task_periods_ns: &[],
            expected_next_tick_ns: None,
        },
        // Single task with 1 second period
        NextTickTestCase {
            task_periods_ns: &[1 * NANOS_PER_SEC],
            expected_next_tick_ns: Some(1 * NANOS_PER_SEC),
        },
        // Two tasks: 1s and 2s periods, returns 1s
        NextTickTestCase {
            task_periods_ns: &[1 * NANOS_PER_SEC, 2 * NANOS_PER_SEC],
            expected_next_tick_ns: Some(1 * NANOS_PER_SEC),
        },
        // Two tasks: 2s and 1.5s periods, returns 1.5s
        NextTickTestCase {
            task_periods_ns: &[2 * NANOS_PER_SEC, 1500 * NANOS_PER_MS],
            expected_next_tick_ns: Some(1500 * NANOS_PER_MS),
        },
    ];

    for case in test_cases {
        let mut scheduler = Scheduler::default();

        for &period_ns in case.task_periods_ns {
            let task = Task {
                task_type: TaskType::Periodic {
                    period: Duration::from_nanos(period_ns),
                },
                anchored: false,
                priority: 0,
                name: None,
                on_execute: None,
                on_miss: None,
            };
            scheduler.add_task(task, 0);
        }

        let next_tick = scheduler.calculate_next_tick();
        let expected = case.expected_next_tick_ns.map(Duration::from_nanos);

        assert_eq!(
            next_tick, expected,
            "Failed for periods {:?}: expected {:?}, got {:?}",
            case.task_periods_ns, expected, next_tick
        );
    }
}

/// A single step in a scheduler cycle test.
struct CycleStep {
    /// Duration until this tick fires (in nanoseconds)
    tick_duration_ns: u64,
    /// Indices of tasks that should execute at this tick
    expected_task_indices: &'static [usize],
}

/// Test case for verifying a full periodic cycle.
struct CycleTestCase {
    /// Periods of tasks to add (in nanoseconds)
    task_periods_ns: &'static [u64],
    /// Sequence of expected tick events in the cycle
    expected_cycle: &'static [CycleStep],
}

#[test]
fn test_scheduler_cycle() {
    let test_cases: &[CycleTestCase] = &[
        // Single 1s task: fires every 1s
        CycleTestCase {
            task_periods_ns: &[1 * NANOS_PER_SEC],
            expected_cycle: &[
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0],
                },
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0],
                },
            ],
        },
        // 1s and 2s tasks: t=1s task0 fires, t=2s both fire
        CycleTestCase {
            task_periods_ns: &[1 * NANOS_PER_SEC, 2 * NANOS_PER_SEC],
            expected_cycle: &[
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0],
                },
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0, 1],
                },
            ],
        },
        // 1s and 1.5s tasks: full cycle until LCM (3s)
        CycleTestCase {
            task_periods_ns: &[1 * NANOS_PER_SEC, 1500 * NANOS_PER_MS],
            expected_cycle: &[
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0],
                }, // t=1s
                CycleStep {
                    tick_duration_ns: 500 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=1.5s
                CycleStep {
                    tick_duration_ns: 500 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=2s
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0, 1],
                }, // t=3s (LCM)
            ],
        },
        // 2s and 1.5s tasks: full cycle until LCM (6s)
        CycleTestCase {
            task_periods_ns: &[2 * NANOS_PER_SEC, 1500 * NANOS_PER_MS],
            expected_cycle: &[
                CycleStep {
                    tick_duration_ns: 1500 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=1.5s
                CycleStep {
                    tick_duration_ns: 500 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=2s
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[1],
                }, // t=3s
                CycleStep {
                    tick_duration_ns: 1 * NANOS_PER_SEC,
                    expected_task_indices: &[0],
                }, // t=4s
                CycleStep {
                    tick_duration_ns: 500 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=4.5s
                CycleStep {
                    tick_duration_ns: 1500 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=6s (LCM)
            ],
        },
        // 0.8s, 1.2s, and 1.4s tasks: full cycle until LCM (16.8s)
        CycleTestCase {
            task_periods_ns: &[800 * NANOS_PER_MS, 1200 * NANOS_PER_MS, 1400 * NANOS_PER_MS],
            expected_cycle: &[
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=0.8s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=1.2s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=1.4s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=1.6s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=2.4s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=2.8s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=3.2s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=3.6s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=4.0s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=4.2s
                CycleStep {
                    tick_duration_ns: 600 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=4.8s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0, 2],
                }, // t=5.6s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=6.0s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=6.4s
                CycleStep {
                    tick_duration_ns: 600 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=7.0s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=7.2s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=8.0s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[1, 2],
                }, // t=8.4s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=8.8s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=9.6s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=9.8s
                CycleStep {
                    tick_duration_ns: 600 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=10.4s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=10.8s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0, 2],
                }, // t=11.2s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=12.0s
                CycleStep {
                    tick_duration_ns: 600 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=12.6s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=12.8s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=13.2s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=13.6s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=14.0s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1],
                }, // t=14.4s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=15.2s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[2],
                }, // t=15.4s
                CycleStep {
                    tick_duration_ns: 200 * NANOS_PER_MS,
                    expected_task_indices: &[1],
                }, // t=15.6s
                CycleStep {
                    tick_duration_ns: 400 * NANOS_PER_MS,
                    expected_task_indices: &[0],
                }, // t=16.0s
                CycleStep {
                    tick_duration_ns: 800 * NANOS_PER_MS,
                    expected_task_indices: &[0, 1, 2],
                }, // t=16.8s (LCM)
            ],
        },
    ];

    for case in test_cases {
        let mut scheduler = Scheduler::default();

        // Add tasks with names matching their index
        for (i, &period_ns) in case.task_periods_ns.iter().enumerate() {
            let task = Task {
                task_type: TaskType::Periodic {
                    period: Duration::from_nanos(period_ns),
                },
                anchored: false,
                priority: 0,
                name: Some(i.to_string()),
                on_execute: None,
                on_miss: None,
            };
            scheduler.add_task(task, 0);
        }

        // Run through the expected cycle twice to verify no drift or weird behavior
        for cycle in 0..2 {
            for (step_idx, step) in case.expected_cycle.iter().enumerate() {
                let next_tick = scheduler.calculate_next_tick();
                let expected_duration = Duration::from_nanos(step.tick_duration_ns);

                assert_eq!(
                    next_tick,
                    Some(expected_duration),
                    "Periods {:?} cycle {} step {}: expected tick {:?}, got {:?}",
                    case.task_periods_ns,
                    cycle,
                    step_idx,
                    expected_duration,
                    next_tick
                );

                // Advance time and get ready tasks
                let ready_tasks = scheduler.tick(expected_duration);
                let ready_indices: Vec<usize> = ready_tasks
                    .iter()
                    .map(|t| t.name.as_ref().unwrap().parse().unwrap())
                    .collect();

                assert_eq!(
                    ready_indices,
                    step.expected_task_indices,
                    "Periods {:?} cycle {} step {}: expected tasks {:?}, got {:?}",
                    case.task_periods_ns,
                    cycle,
                    step_idx,
                    step.expected_task_indices,
                    ready_indices
                );
            }
        }
    }
}

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
        let mut scheduler = Scheduler::default();

        let task = Task {
            task_type: TaskType::Periodic {
                period: Duration::from_nanos(case.period_ns),
            },
            anchored: true,
            priority: 0,
            name: Some("anchored".to_string()),
            on_execute: None,
            on_miss: None,
        };

        scheduler.add_task(task, case.current_offset_ns as u128);

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
    let mut scheduler = Scheduler::default();

    // Scenario: Clock starts at 200ms into the current second
    // Add first task at scheduler time 0 (clock at 200ms)
    let task0 = Task {
        task_type: TaskType::Periodic {
            period: Duration::from_nanos(1 * NANOS_PER_SEC),
        },
        anchored: true,
        priority: 0,
        name: Some("0".to_string()),
        on_execute: None,
        on_miss: None,
    };
    scheduler.add_task(task0, 200 * NANOS_PER_MS as u128);

    // Advance 300ms (clock now at 500ms into the second)
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0); // No tasks fire yet

    // Add second task at scheduler time 300ms (clock at 500ms)
    let task1 = Task {
        task_type: TaskType::Periodic {
            period: Duration::from_nanos(1 * NANOS_PER_SEC),
        },
        anchored: true,
        priority: 0,
        name: Some("1".to_string()),
        on_execute: None,
        on_miss: None,
    };
    scheduler.add_task(task1, 500 * NANOS_PER_MS as u128);

    // Both tasks should now fire at the 1-second boundary
    // Task 0: added at epoch 200ms, fires at 1000ms (800ms from scheduler start)
    // Task 1: added at epoch 500ms, fires at 1000ms (800ms from scheduler start, 500ms from add)
    // Next tick should be 500ms away (to reach 800ms total)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(500 * NANOS_PER_MS)));

    // Execute - both tasks fire together at the 1-second boundary
    let ready = scheduler.tick(Duration::from_nanos(500 * NANOS_PER_MS));
    assert_eq!(ready.len(), 2);

    // Now both are synchronized - next tick at 1s, both fire
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(1 * NANOS_PER_SEC)));

    let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
    assert_eq!(ready.len(), 2);
}

#[test]
fn test_three_anchored_tasks() {
    let mut scheduler = Scheduler::default();

    // Scenario: Add three tasks at different times as clock advances
    // Clock starts at 100ms into the current second

    // Task 0: added at scheduler time 0ms (clock at 100ms)
    let task0 = Task {
        task_type: TaskType::Periodic {
            period: Duration::from_nanos(1 * NANOS_PER_SEC),
        },
        anchored: true,
        priority: 0,
        name: Some("0".to_string()),
        on_execute: None,
        on_miss: None,
    };
    scheduler.add_task(task0, 100 * NANOS_PER_MS as u128);

    // Advance 300ms (clock now at 400ms)
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0);

    // Task 1: added at scheduler time 300ms (clock at 400ms)
    let task1 = Task {
        task_type: TaskType::Periodic {
            period: Duration::from_nanos(1 * NANOS_PER_SEC),
        },
        anchored: true,
        priority: 0,
        name: Some("1".to_string()),
        on_execute: None,
        on_miss: None,
    };
    scheduler.add_task(task1, 400 * NANOS_PER_MS as u128);

    // Advance 300ms more (clock now at 700ms)
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0);

    // Task 2: added at scheduler time 600ms (clock at 700ms)
    let task2 = Task {
        task_type: TaskType::Periodic {
            period: Duration::from_nanos(1 * NANOS_PER_SEC),
        },
        anchored: true,
        priority: 0,
        name: Some("2".to_string()),
        on_execute: None,
        on_miss: None,
    };
    scheduler.add_task(task2, 700 * NANOS_PER_MS as u128);

    // All three tasks should fire at the next 1-second boundary
    // Task 0: added at epoch 100ms, next boundary at 1000ms (900ms from start)
    // Task 1: added at epoch 400ms, next boundary at 1000ms (900ms from start)
    // Task 2: added at epoch 700ms, next boundary at 1000ms (900ms from start)
    // Currently at 600ms, so 300ms until all fire
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(300 * NANOS_PER_MS)));

    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 3);

    // Verify all subsequent ticks are at 1s intervals with all three firing
    for _ in 0..3 {
        let next_tick = scheduler.calculate_next_tick();
        assert_eq!(next_tick, Some(Duration::from_nanos(1 * NANOS_PER_SEC)));
        let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
        assert_eq!(ready.len(), 3);
    }
}
