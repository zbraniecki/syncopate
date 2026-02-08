# Syncopate

A hierarchical, power-aware task scheduler for Rust applications requiring precise timing control.

## Overview

Syncopate provides a flexible scheduler for managing periodic tasks with configurable execution windows. It's designed for applications that need:

- **Deterministic timing**: Schedule tasks to run at specific intervals
- **Execution windows**: Define acceptable time ranges for task execution (early/on-time/late detection)
- **Power efficiency**: Idle durations calculated to minimize CPU wakeups
- **Hierarchical organization**: Group and manage related tasks (planned)

## Quick Start

Add syncopate to your `Cargo.toml`:

```toml
[dependencies]
syncopate = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Example

```rust
use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskId, TaskType},
};

#[tokio::main]
async fn main() {
    // Build a scheduler with min/max period bounds
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(100))
        .max_period(Duration::from_secs(2))
        .build();

    // Spawn the scheduler loop
    tokio::spawn(async move {
        loop {
            // Poll the scheduler to get the next plan
            let plan = scheduler.poll();

            // Handle due tasks
            for task in &plan.due_tasks {
                println!("Executing task {:?}", task.id);
                // ... your task logic here ...
            }
            
            // Mark completed tasks
            let completed: Vec<_> = plan.due_tasks.iter().map(|t| t.id).collect();
            scheduler.mark_completed(&completed);

            // Handle missed tasks
            for miss in &plan.missed_tasks {
                eprintln!("Task {:?} missed its window", miss.id);
            }

            // Sleep for calculated idle duration
            if plan.idle_duration > Duration::ZERO {
                tokio::time::sleep(plan.idle_duration).await;
            }
        }
    });

    // Add a periodic task from the main thread
    let task_id = handle
        .add_task(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_secs(1),
                window_before: Duration::from_millis(50),
                window_after: Duration::from_millis(50),
            },
            priority: 0,
            name: Some("my_task".into()),
        })
        .expect("Failed to add task");

    println!("Scheduled task {:?}", task_id);
}
```

## Core Concepts

### Periodic Tasks

Tasks are defined with:
- **period**: How often the task should execute
- **window_before**: How early the task can execute before its ideal time
- **window_after**: How late the task can execute after its ideal time
- **priority**: Lower values = higher priority for conflict resolution

### Scheduler Bounds

The scheduler enforces minimum and maximum periods:
- Tasks with periods below `min_period` are rejected
- Tasks with periods above `max_period` are rejected
- When no tasks are scheduled, the scheduler sleeps for `max_period`

### Execution Categories

Tasks are classified based on actual vs. ideal timing:
- **Early**: Executed before `window_before`
- **On-Time**: Executed within `[ideal - window_before, ideal + window_after]`
- **Late**: Executed after `window_after`
- **Missed**: Never executed within the window

## Architecture

Syncopate uses a poll-based design:

1. **SchedulerLoop**: Core scheduling logic, single-threaded owner
2. **SchedulerHandle**: Cloneable handle for adding tasks from any thread
3. **WakeupPlan**: Output of each poll containing due tasks and recommended idle duration
4. **BinaryHeap**: Tasks ordered by deadline for efficient scheduling

## Benchmarks

Run the benchmark example to measure timing accuracy:

```bash
cargo run --example benchmark -- --duration 10s --task-period 1s
```

## Planned Features

Planned features include:

### Core Enhancements
- [ ] **One-Shot Tasks**: Single-execution tasks with monotonic or wall-clock deadlines
- [ ] **Task Dependencies**: Express "task B must run after task A" with DAG-based scheduling
- [ ] **Task Groups**: Atomic execution - all tasks in a group appear in `due_tasks` together
- [ ] **Dynamic Priority Adjustment**: User-defined priority functions and adaptive priority based on miss rates
- [ ] **Task Cancellation**: Graceful and immediate task termination

### Advanced Scheduling
- [ ] **Efficiency Tier**: Coalescing algorithm for power optimization (weighted interval sweep)
- [ ] **Hierarchical Sub-Schedulers**: Parent-child scheduler relationships with period constraints
- [ ] **Period Negotiation**: Request/response protocol for global period optimization
- [ ] **Wall-Clock Support**: `Deadline::WallClock(SystemTime)` resolution with clock-jump detection

### Observability & Integration
- [ ] **Tracing Integration**: `tracing` crate for observability with scheduling spans
- [ ] **Metrics Export**: Prometheus metrics and Grafana dashboards
- [ ] **Energy Profiling**: Measure actual power consumption per coalescing strategy
- [ ] **Configuration Files**: YAML/JSON task definitions

### Performance & Platform Support
- [ ] **no_std Support**: Target embedded systems (Cortex-M) without `std::time`
- [ ] **Arena Allocator**: Avoid per-task heap allocation for better performance
- [ ] **SIMD Coalescing**: Vectorize interval sweep for very large task sets (10,000+)
- [ ] **Distributed Scheduling**: Multi-process coordination via shared memory

## License

MIT OR Apache-2.0
