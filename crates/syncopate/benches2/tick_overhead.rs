use divan::{Bencher, black_box};
use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::{TaskBuilder, Window};

fn main() {
    divan::main();
}

/// Simulates a realistic scheduler over a given duration
/// Returns total number of wakeups
fn simulate_scheduler(tasks: Vec<(Duration, Window)>, simulation_duration: Duration) -> usize {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

    // Add initial tasks
    for (i, (period, window)) in tasks.iter().enumerate() {
        let task = TaskBuilder::every(*period, *window)
            .name(format!("task_{}", i))
            .build()
            .unwrap();
        scheduler.add_task(task).unwrap();
    }

    let mut current_time = epoch;
    let end_time = epoch + simulation_duration;
    let mut wakeup_count = 0;

    // Simulate scheduler loop
    while current_time < end_time {
        // Calculate optimal next wake point
        let next_tick_duration = match scheduler.calculate_next_tick() {
            Some(d) if d > Duration::ZERO => d,
            _ => Duration::from_millis(1), // Minimum tick
        };

        // Advance to next wake point
        current_time += next_tick_duration;
        if current_time >= end_time {
            break;
        }

        scheduler.advance_time(current_time);
        let fired = scheduler.tick(next_tick_duration);

        if !fired.is_empty() {
            wakeup_count += 1;
        }
    }

    wakeup_count
}

/// Baseline: 10 simple relative periodic tasks
/// Measures minimal per-tick overhead with a light load
#[divan::bench]
fn tick_10_relative_tasks(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
            let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

            // Add 10 tasks with different periods
            for i in 1..=10 {
                let task = TaskBuilder::every(Duration::from_millis(100 * i), Window::ZERO)
                    .name(format!("task_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            (scheduler, epoch)
        })
        .bench_values(|(mut scheduler, mut current_time)| {
            // Advance time and tick
            current_time += Duration::from_millis(10);
            scheduler.advance_time(current_time);
            black_box(scheduler.tick(Duration::from_millis(10)).len());
        });
}

/// Medium load: 100 relative periodic tasks
/// Tests scaling behavior with many tasks
#[divan::bench]
fn tick_100_relative_tasks(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
            let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

            // Add 100 tasks with varying periods
            for i in 1..=100 {
                let task = TaskBuilder::every(Duration::from_millis(50 + i * 10), Window::ZERO)
                    .name(format!("task_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            (scheduler, epoch)
        })
        .bench_values(|(mut scheduler, mut current_time)| {
            current_time += Duration::from_millis(10);
            scheduler.advance_time(current_time);
            black_box(scheduler.tick(Duration::from_millis(10)).len());
        });
}

/// Heavy absolute: 50 absolute periodic tasks with different periods/offsets
/// Tests more expensive absolute timing calculations
#[divan::bench]
fn tick_50_absolute_tasks(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
            let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

            // Add 25 boundary-aligned tasks
            for i in 1..=25 {
                let task =
                    TaskBuilder::every_at_boundary(Duration::from_millis(100 * i), Window::ZERO)
                        .name(format!("boundary_{}", i))
                        .build()
                        .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // Add 25 offset tasks
            for i in 1..=25 {
                let period = Duration::from_millis(200 * i);
                let offset = Duration::from_millis(50 * i);
                let task = TaskBuilder::every_with_offset(period, offset, Window::ZERO)
                    .name(format!("offset_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            (scheduler, epoch)
        })
        .bench_values(|(mut scheduler, mut current_time)| {
            current_time += Duration::from_millis(10);
            scheduler.advance_time(current_time);
            black_box(scheduler.tick(Duration::from_millis(10)).len());
        });
}

/// Mixed workload: 100 tasks with varied types and timing modes
/// Simulates realistic usage with relative/absolute, periodic/one-time tasks
#[divan::bench]
fn tick_100_mixed_tasks(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
            let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

            // 40 relative periodic tasks
            for i in 1..=40 {
                let task = TaskBuilder::every(Duration::from_millis(50 + i * 10), Window::ZERO)
                    .name(format!("rel_periodic_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // 30 absolute periodic tasks (boundary-aligned)
            for i in 1..=30 {
                let task =
                    TaskBuilder::every_at_boundary(Duration::from_millis(100 * i), Window::ZERO)
                        .name(format!("abs_boundary_{}", i))
                        .build()
                        .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // 20 absolute periodic tasks (with offset)
            for i in 1..=20 {
                let period = Duration::from_millis(200 * i);
                let offset = Duration::from_millis(30 * i);
                let task = TaskBuilder::every_with_offset(period, offset, Window::ZERO)
                    .name(format!("abs_offset_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // 10 relative one-time tasks (will fire and be removed)
            for i in 1..=10 {
                let task =
                    TaskBuilder::once_after(Duration::from_millis(500 + i * 100), Window::ZERO)
                        .name(format!("rel_once_{}", i))
                        .build()
                        .unwrap();
                scheduler.add_task(task).unwrap();
            }

            (scheduler, epoch)
        })
        .bench_values(|(mut scheduler, mut current_time)| {
            current_time += Duration::from_millis(10);
            scheduler.advance_time(current_time);
            black_box(scheduler.tick(Duration::from_millis(10)).len());
        });
}

/// Wakeup efficiency: Exact timing (no windows)
/// Measures total wakeups over 10s with exact timing requirements
#[divan::bench]
fn wakeup_efficiency_exact_timing() -> usize {
    // 10 tasks with slightly different periods - no coalescing possible
    let tasks = vec![
        (Duration::from_millis(1000), Window::ZERO), // 1.000s
        (Duration::from_millis(1100), Window::ZERO), // 1.100s
        (Duration::from_millis(1200), Window::ZERO), // 1.200s
        (Duration::from_millis(900), Window::ZERO),  // 0.900s
        (Duration::from_millis(1050), Window::ZERO), // 1.050s
        (Duration::from_millis(1150), Window::ZERO), // 1.150s
        (Duration::from_millis(950), Window::ZERO),  // 0.950s
        (Duration::from_millis(1250), Window::ZERO), // 1.250s
        (Duration::from_millis(850), Window::ZERO),  // 0.850s
        (Duration::from_millis(1300), Window::ZERO), // 1.300s
    ];

    simulate_scheduler(tasks, Duration::from_secs(10))
}

/// Wakeup efficiency: With execution windows
/// Same tasks but with ±50ms windows - should enable coalescing
#[divan::bench]
fn wakeup_efficiency_with_windows() -> usize {
    let window = Window::symmetric(Duration::from_millis(50));

    // Same 10 tasks but with ±50ms flexibility
    let tasks = vec![
        (Duration::from_millis(1000), window),
        (Duration::from_millis(1100), window),
        (Duration::from_millis(1200), window),
        (Duration::from_millis(900), window),
        (Duration::from_millis(1050), window),
        (Duration::from_millis(1150), window),
        (Duration::from_millis(950), window),
        (Duration::from_millis(1250), window),
        (Duration::from_millis(850), window),
        (Duration::from_millis(1300), window),
    ];

    simulate_scheduler(tasks, Duration::from_secs(10))
}

/// Wakeup efficiency: Aggressive windows
/// Same tasks with ±100ms windows - maximum coalescing
#[divan::bench]
fn wakeup_efficiency_aggressive_windows() -> usize {
    let window = Window::symmetric(Duration::from_millis(100));

    let tasks = vec![
        (Duration::from_millis(1000), window),
        (Duration::from_millis(1100), window),
        (Duration::from_millis(1200), window),
        (Duration::from_millis(900), window),
        (Duration::from_millis(1050), window),
        (Duration::from_millis(1150), window),
        (Duration::from_millis(950), window),
        (Duration::from_millis(1250), window),
        (Duration::from_millis(850), window),
        (Duration::from_millis(1300), window),
    ];

    simulate_scheduler(tasks, Duration::from_secs(10))
}

/// Wakeup efficiency: Dynamic task addition
/// Tests adding tasks mid-simulation with windows
#[divan::bench]
fn wakeup_efficiency_dynamic_addition() -> usize {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);
    let window = Window::symmetric(Duration::from_millis(75));

    // Add 3 initial tasks
    for i in 0..3 {
        let task = TaskBuilder::every(Duration::from_millis(1000 + i * 100), window)
            .name(format!("initial_{}", i))
            .build()
            .unwrap();
        scheduler.add_task(task).unwrap();
    }

    let mut current_time = epoch;
    let end_time = epoch + Duration::from_secs(10);
    let mut wakeup_count = 0;
    let mut tasks_added = false;

    // Simulate scheduler loop
    while current_time < end_time {
        let next_tick_duration = match scheduler.calculate_next_tick() {
            Some(d) if d > Duration::ZERO => d,
            _ => Duration::from_millis(1),
        };

        current_time += next_tick_duration;
        if current_time >= end_time {
            break;
        }

        // Add more tasks after 2 seconds
        if !tasks_added && current_time >= epoch + Duration::from_secs(2) {
            for i in 3..7 {
                let task = TaskBuilder::every(Duration::from_millis(900 + i * 50), window)
                    .name(format!("dynamic_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }
            tasks_added = true;
        }

        scheduler.advance_time(current_time);
        let fired = scheduler.tick(next_tick_duration);

        if !fired.is_empty() {
            wakeup_count += 1;
        }
    }

    wakeup_count
}
