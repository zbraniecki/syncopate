use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

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
        let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

        for &period_ns in case.task_periods_ns {
            let task = TaskBuilder::every(Duration::from_nanos(period_ns))
                .build()
                .unwrap();
            scheduler.add_task(task).unwrap();
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
        let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

        // Add tasks with names matching their index
        for (i, &period_ns) in case.task_periods_ns.iter().enumerate() {
            let task = TaskBuilder::every(Duration::from_nanos(period_ns))
                .name(i.to_string())
                .build()
                .unwrap();
            scheduler.add_task(task).unwrap();
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
