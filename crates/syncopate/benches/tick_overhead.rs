use divan::{black_box, Bencher};
use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

fn main() {
    divan::main();
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
                let task = TaskBuilder::every(Duration::from_millis(100 * i))
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
                let task = TaskBuilder::every(Duration::from_millis(50 + i * 10))
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
                let task = TaskBuilder::every_at_boundary(Duration::from_millis(100 * i))
                    .name(format!("boundary_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // Add 25 offset tasks
            for i in 1..=25 {
                let period = Duration::from_millis(200 * i);
                let offset = Duration::from_millis(50 * i);
                let task = TaskBuilder::every_with_offset(period, offset)
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
                let task = TaskBuilder::every(Duration::from_millis(50 + i * 10))
                    .name(format!("rel_periodic_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // 30 absolute periodic tasks (boundary-aligned)
            for i in 1..=30 {
                let task = TaskBuilder::every_at_boundary(Duration::from_millis(100 * i))
                    .name(format!("abs_boundary_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // 20 absolute periodic tasks (with offset)
            for i in 1..=20 {
                let period = Duration::from_millis(200 * i);
                let offset = Duration::from_millis(30 * i);
                let task = TaskBuilder::every_with_offset(period, offset)
                    .name(format!("abs_offset_{}", i))
                    .build()
                    .unwrap();
                scheduler.add_task(task).unwrap();
            }

            // 10 relative one-time tasks (will fire and be removed)
            for i in 1..=10 {
                let task = TaskBuilder::once_after(Duration::from_millis(500 + i * 100))
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
