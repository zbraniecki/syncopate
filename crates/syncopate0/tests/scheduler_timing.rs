//! Integration tests for scheduler timing calculations.
//!
//! These tests verify that the scheduler correctly calculates idle durations
//! based on task scheduling without testing the actual runtime loop.

use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

#[test]
fn test_no_tasks_returns_max_period() {
    // When no tasks are scheduled, poll() should return max_period
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(10))
        .max_period(Duration::from_secs(2))
        .build();

    let idle = scheduler.poll();
    assert_eq!(idle, Duration::from_secs(2));
}

#[test]
fn test_single_task_returns_period() {
    // When a single task is scheduled, poll() should return a duration
    // close to the task period (accounting for timing precision)
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .build();

    // Add a task with 16ms period
    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(16),
                window_before: Duration::from_millis(1),
                window_after: Duration::from_millis(1),
                anchored: false,
            },
            priority: 0,
            name: Some("test_task".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    let idle = scheduler.poll();

    // The idle duration should be close to 16ms (within a small tolerance)
    // It may be slightly less due to timing precision and scheduling overhead
    assert!(
        idle >= Duration::from_millis(15) && idle <= Duration::from_millis(17),
        "Expected idle duration around 16ms, got {:?}",
        idle
    );
}

#[test]
fn test_multiple_tasks_returns_shortest_period() {
    // When multiple tasks are scheduled, poll() should return the duration
    // until the next task is due (the one with the shortest remaining time)
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .build();

    // Add task with 100ms period
    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(100),
                window_before: Duration::from_millis(5),
                window_after: Duration::from_millis(5),
                anchored: false,
            },
            priority: 0,
            name: Some("slow_task".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    // Add task with 16ms period
    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(16),
                window_before: Duration::from_millis(1),
                window_after: Duration::from_millis(1),
                anchored: false,
            },
            priority: 0,
            name: Some("fast_task".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    let idle = scheduler.poll();

    // Should return duration close to 16ms (the shorter period)
    assert!(
        idle >= Duration::from_millis(15) && idle <= Duration::from_millis(17),
        "Expected idle duration around 16ms, got {:?}",
        idle
    );
}

#[test]
fn test_task_execution_reschedules() {
    // After a task executes, the next idle duration should be recalculated
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .build();

    scheduler
        .add_task_local(
            TaskConfig {
                task_type: TaskType::Periodic {
                    period: Duration::from_millis(16),
                    window_before: Duration::from_millis(1),
                    window_after: Duration::from_millis(1),
                    anchored: false,
                },
                priority: 0,
                name: Some("test_task".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(|_exec, _ctx| {
                // Task callback - just record that it executed
            }),
        )
        .expect("Failed to add task");

    // First poll - task not due yet
    let idle1 = scheduler.poll();
    assert!(idle1 >= Duration::from_millis(15));

    // Sleep and poll again after the task should have executed
    std::thread::sleep(idle1 + Duration::from_millis(1));
    let idle2 = scheduler.poll();

    // After execution, should schedule next period (~16ms again)
    assert!(
        idle2 >= Duration::from_millis(15) && idle2 <= Duration::from_millis(17),
        "Expected idle duration around 16ms after execution, got {:?}",
        idle2
    );
}

#[test]
fn test_sleep_compensation_reduces_idle() {
    // With sleep compensation, the idle duration should be reduced
    // by the compensation amount (but not below window_start)
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .sleep_compensation(Duration::from_millis(2))
        .build();

    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(16),
                window_before: Duration::from_millis(1),
                window_after: Duration::from_millis(1),
                anchored: false,
            },
            priority: 0,
            name: Some("test_task".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    let idle = scheduler.poll();

    // With 2ms compensation, idle should be around 14ms (16ms - 2ms)
    // but clamped to not wake before the window starts
    assert!(
        idle >= Duration::from_millis(13) && idle <= Duration::from_millis(15),
        "Expected idle duration around 14ms with compensation, got {:?}",
        idle
    );
}

#[test]
fn test_window_before_limits_early_wakeup() {
    // Even with large sleep compensation, wakeup should not happen
    // before the window_before boundary
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .sleep_compensation(Duration::from_millis(10)) // Large compensation
        .build();

    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(16),
                window_before: Duration::from_millis(2), // Window starts 2ms before deadline
                window_after: Duration::from_millis(2),
                anchored: false,
            },
            priority: 0,
            name: Some("test_task".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    let idle = scheduler.poll();

    // With 10ms compensation, naive sleep would be 6ms (16ms - 10ms)
    // But window_before=2ms means earliest wakeup is 14ms (16ms - 2ms)
    // So idle should be clamped to ~14ms
    assert!(
        idle >= Duration::from_millis(13) && idle <= Duration::from_millis(15),
        "Expected idle duration clamped to window_before (~14ms), got {:?}",
        idle
    );
}

#[test]
fn test_interval_mode_reschedules_from_now() {
    // In interval mode (anchored=false), after a task executes late,
    // the next deadline should be relative to the actual execution time,
    // not the original deadline
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .build();

    scheduler
        .add_task_local(
            TaskConfig {
                task_type: TaskType::Periodic {
                    period: Duration::from_millis(16),
                    window_before: Duration::from_millis(1),
                    window_after: Duration::from_millis(1),
                    anchored: false, // interval mode
                },
                priority: 0,
                name: Some("test_task".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(|_exec, _ctx| {
                // Task executes
            }),
        )
        .expect("Failed to add task");

    // First poll
    let idle1 = scheduler.poll();

    // Sleep long enough for task to execute (and potentially be slightly late)
    std::thread::sleep(idle1 + Duration::from_millis(5));

    // Second poll - task should execute
    let idle2 = scheduler.poll();

    // Next idle should still be close to 16ms (rescheduled from execution time)
    assert!(
        idle2 >= Duration::from_millis(15) && idle2 <= Duration::from_millis(17),
        "Expected idle duration around 16ms after late execution in interval mode, got {:?}",
        idle2
    );
}

#[test]
fn test_anchored_mode_maintains_grid() {
    // In anchored mode (anchored=true), tasks maintain their fixed grid
    // even after late execution
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .build();

    scheduler
        .add_task_local(
            TaskConfig {
                task_type: TaskType::Periodic {
                    period: Duration::from_millis(16),
                    window_before: Duration::from_millis(5),
                    window_after: Duration::from_millis(5),
                    anchored: true, // anchored mode
                },
                priority: 0,
                name: Some("test_task".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(|_exec, _ctx| {
                // Task executes
            }),
        )
        .expect("Failed to add task");

    // First poll
    let idle1 = scheduler.poll();

    // Sleep a bit less than required
    std::thread::sleep(idle1 - Duration::from_millis(2));

    // Poll again - task should execute this time
    let idle2 = scheduler.poll();

    // In anchored mode, next deadline is on the fixed grid
    // Should still be close to 16ms
    assert!(
        idle2 >= Duration::from_millis(14) && idle2 <= Duration::from_millis(18),
        "Expected idle duration around 16ms in anchored mode, got {:?}",
        idle2
    );
}

#[test]
fn test_priority_does_not_affect_timing() {
    // Task priority should affect execution order when tasks are due
    // at the same time, but should not affect idle duration calculations
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(1))
        .max_period(Duration::from_secs(5))
        .build();

    // Add high priority task with 100ms period
    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(100),
                window_before: Duration::from_millis(5),
                window_after: Duration::from_millis(5),
                anchored: false,
            },
            priority: 0, // high priority
            name: Some("high_priority".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    // Add low priority task with 16ms period
    scheduler
        .add_task_local(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(16),
                window_before: Duration::from_millis(1),
                window_after: Duration::from_millis(1),
                anchored: false,
            },
            priority: 10, // low priority
            name: Some("low_priority".into()),
            on_execute: None,
            on_miss: None,
        })
        .expect("Failed to add task");

    let idle = scheduler.poll();

    // Should wake for the soonest task regardless of priority (16ms task)
    assert!(
        idle >= Duration::from_millis(15) && idle <= Duration::from_millis(17),
        "Expected idle duration determined by soonest task (16ms), got {:?}",
        idle
    );
}
