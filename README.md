# Syncopate

A hierarchical, power-aware task scheduler for Rust applications requiring precise timing control.

## Overview

Syncopate provides a flexible scheduler for managing periodic tasks with configurable execution windows. It's designed for applications that need:

- **Deterministic timing**: Schedule tasks to run at specific intervals
- **Execution windows**: Define acceptable time ranges for task execution (early/on-time/late detection)
- **Power efficiency**: Idle durations calculated to minimize CPU wakeups
- **Flexible contexts**: Share state between tasks using custom context types
- **Runtime flexibility**: Support both single-threaded and multi-threaded async runtimes

## Quick Start

Add syncopate to your `Cargo.toml`:

```toml
[dependencies]
syncopate = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Examples

### Simple Callback-Based Usage

The callback-based API provides a clean, minimal interface where tasks execute automatically:

```rust
use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

#[tokio::main]
async fn main() {
    // Build a scheduler with default (unit) context
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_millis(100))
        .max_period(Duration::from_secs(2))
        .build();

    // Spawn the scheduler loop - just poll and sleep!
    tokio::spawn(async move {
        loop {
            let idle = scheduler.poll();
            tokio::time::sleep(idle).await;
        }
    });

    // Add tasks with execution callbacks
    handle.add_task(
        TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_secs(1),
                window_before: Duration::from_millis(50),
                window_after: Duration::from_millis(50),
            },
            priority: 0,
            name: Some("sensor".into()),
            on_execute: None,
            on_miss: None,
        }
        .with_executor(|exec, _ctx| {
            println!("Sensor reading: drift={:?}", exec.drift);
            // Your task logic here - keep it fast!
        })
        .with_miss_handler(|miss, _ctx| {
            eprintln!("Sensor task missed {} times!", miss.miss_count);
        })
    ).expect("Failed to add task");
}
```

**Key improvements:**
- `poll()` returns just `Duration` (how long to sleep)
- No `mark_completed()` calls needed
- No `WakeupPlan` struct to inspect
- Simple loop: `loop { sleep(scheduler.poll()).await; }`

### Using Context for Shared State

Define a custom context to share state between tasks and your application:

```rust
use std::sync::{Arc, Mutex};
use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

// Define your application context
#[derive(Clone)]
struct AppContext {
    execution_count: Arc<Mutex<usize>>,
    total_drift: Arc<Mutex<Duration>>,
}

#[tokio::main]
async fn main() {
    // Create the context
    let context = AppContext {
        execution_count: Arc::new(Mutex::new(0)),
        total_drift: Arc::new(Mutex::new(Duration::ZERO)),
    };

    // Build scheduler with the context
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .with_context(context.clone())
        .build();

    // Spawn the scheduler loop
    tokio::spawn(async move {
        loop {
            let idle = scheduler.poll();
            tokio::time::sleep(idle).await;
        }
    });

    // Add tasks that use the context
    handle.add_task(
        TaskConfig::<AppContext> {
            task_type: TaskType::Periodic {
                period: Duration::from_secs(1),
                window_before: Duration::from_millis(50),
                window_after: Duration::from_millis(50),
            },
            priority: 0,
            name: Some("counter".into()),
            on_execute: None,
            on_miss: None,
        }
        .with_executor(|exec, ctx| {
            // Update shared state through the context
            let mut count = ctx.execution_count.lock().unwrap();
            *count += 1;

            let mut drift = ctx.total_drift.lock().unwrap();
            *drift += exec.drift;
        })
    ).expect("Failed to add task");

    // Access context from your application
    tokio::time::sleep(Duration::from_secs(5)).await;
    let count = *context.execution_count.lock().unwrap();
    println!("Total executions: {}", count);
}
```

### Single-Threaded (Local) Usage

For single-threaded async runtimes (like `tokio::task::spawn_local`), you can use `Rc<RefCell<T>>` instead of `Arc<Mutex<T>>`:

```rust
use std::rc::Rc;
use std::cell::RefCell;
use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

// Context with Rc/RefCell for single-threaded use
struct LocalContext {
    count: Rc<RefCell<usize>>,
}

#[tokio::main]
async fn main() {
    let local = tokio::task::LocalSet::new();

    local.run_until(async move {
        let context = LocalContext {
            count: Rc::new(RefCell::new(0)),
        };

        // Use build_local() for single-threaded usage
        let mut scheduler = SchedulerBuilder::new()
            .with_context(context)
            .build_local();

        // Add tasks directly (no handle needed)
        scheduler.add_task_local(
            TaskConfig {
                task_type: TaskType::Periodic {
                    period: Duration::from_secs(1),
                    window_before: Duration::from_millis(50),
                    window_after: Duration::from_millis(50),
                },
                priority: 0,
                name: Some("local_task".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(|_exec, ctx| {
                *ctx.count.borrow_mut() += 1;
            })
        ).expect("Failed to add task");

        // Spawn the scheduler loop locally
        tokio::task::spawn_local(async move {
            loop {
                let idle = scheduler.poll();
                tokio::time::sleep(idle).await;
            }
        });

        // Your app logic here
        tokio::time::sleep(Duration::from_secs(5)).await;
    }).await;
}
```

## Core Concepts

### Callback-Based Execution

Tasks execute automatically via callbacks during `poll()`:
- **`on_execute`**: Called when the task is executed (receives `TaskExecution` with drift info and `&Context`)
- **`on_miss`**: Called when the task misses its window (receives `TaskMiss` with miss count and `&Context`)
- Callbacks are synchronous - keep them fast to avoid blocking the scheduler
- For async work, spawn tasks from within the callback

### Custom Contexts

Define your own context type to share state:
- **Multi-threaded**: Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>`, requires `Send + Sync`
- **Single-threaded**: Use `Rc<RefCell<T>>`, no `Send + Sync` required
- **Flexible structure**: Define any fields you need (counters, queues, configuration, etc.)
- **Zero overhead**: Unit context `()` when no shared state is needed

### Periodic Tasks

Tasks are defined with:
- **period**: How often the task should execute
- **window_before**: How early the task can execute before its ideal time
- **window_after**: How late the task can execute after its ideal time
- **priority**: Lower values = higher priority for conflict resolution
- **on_execute**: Optional callback for automatic execution (receives `TaskExecution` and `&Context`)
- **on_miss**: Optional callback for deadline violations (receives `TaskMiss` and `&Context`)

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

## API Design

### Multi-Threaded Usage (with Handle)

```rust
let (handle, mut scheduler) = SchedulerBuilder::new()
    .with_context(context.clone())
    .build();

// Add tasks from any thread via the handle
handle.add_task(config)?;

// Run scheduler loop
loop {
    let idle = scheduler.poll();
    tokio::time::sleep(idle).await;
}
```

**Requirements:**
- Context must implement `Send + Sync + 'static`
- Callbacks must be `Send + Sync`
- Use `Arc<Mutex<T>>` for shared state

### Single-Threaded Usage (no Handle)

```rust
let mut scheduler = SchedulerBuilder::new()
    .with_context(context)
    .build_local();

// Add tasks directly
scheduler.add_task_local(config)?;

// Run scheduler loop
loop {
    let idle = scheduler.poll();
    tokio::time::sleep(idle).await;
}
```

**Benefits:**
- No `Send + Sync` requirements
- Can use `Rc<RefCell<T>>`
- Simpler for single-threaded runtimes

## Architecture

Syncopate uses a poll-based design:

1. **SchedulerLoop**: Core scheduling logic, single-threaded owner
2. **SchedulerHandle**: Cloneable handle for adding tasks from any thread (optional)
3. **Context**: User-defined type shared between tasks and application
4. **BinaryHeap**: Tasks ordered by deadline for efficient scheduling

## Benchmarks

Run the benchmark example to measure timing accuracy:

```bash
cargo run --example benchmark -- --duration 10s --task-period 1s
```

The benchmark demonstrates using context to collect execution statistics.

## Planned Features

The current implementation provides core callback-based scheduling with context support. Future enhancements are planned based on the [High-Level Design document](scheduler-hld.md):

### Core Enhancements

- [ ] **One-Shot Tasks**: Single-execution tasks with monotonic or wall-clock deadlines
  - `TaskType::OneShot` with `Deadline::Monotonic(Instant)` and `Deadline::WallClock(SystemTime)`
  - Automatic removal after execution
  - Clock-jump detection for wall-clock deadlines

- [ ] **Priority Lanes**: Multi-level priority queues with EDF within each level
  - Configurable number of priority levels
  - Optional priority aging to prevent starvation
  - Priority-first, deadline-second scheduling

- [ ] **Task Lifecycle Management**: Enhanced task control
  - Pause/resume tasks without removing them
  - Modify task configuration via `TaskPatch`
  - Task removal by ID

- [ ] **Task Dependencies**: Express "task B must run after task A"
  - DAG-based scheduling within a single poll cycle
  - Dependency validation at task creation

- [ ] **Task Groups**: Atomic execution
  - All tasks in a group fire together or not at all
  - Useful for coordinated multi-task operations

### Advanced Scheduling

- [ ] **Two-Tier Architecture**: Separate precision and efficiency modes
  - **Precision Tier**: O(log n) EDF peek for sub-millisecond scheduling (10-100 μs periods)
  - **Efficiency Tier**: O(n log n) weighted interval coalescing for power-saving (1 ms+ periods)
  - Tier selection at scheduler construction

- [ ] **Task Coalescing** (Efficiency Tier): Batch tasks within overlapping windows
  - Weighted interval sweep algorithm (O(n log n))
  - Priority-aware coalescing decisions
  - Minimize wakeups for power management

- [ ] **Hierarchical Sub-Schedulers**: Parent-child scheduler relationships
  - Tree of schedulers with period constraints (child period ≥ parent period)
  - Task isolation between scheduler levels
  - Multi-level hierarchy support

- [ ] **Period Negotiation**: Sub-schedulers can request parent period changes
  - Request/response protocol via channels
  - Global period recomputation using GCD on integer nanoseconds
  - Per-child allow/deny policies

### Observability & Integration

- [ ] **Tracing Integration**: `tracing` crate for observability
  - Scheduling spans for each poll cycle
  - Task execution and miss events
  - Performance instrumentation

- [ ] **Metrics Export**: Production monitoring
  - Prometheus metrics endpoint
  - Grafana dashboards
  - Key metrics: task execution counts, miss rates, coalescing efficiency, idle duration

- [ ] **Statistics API**: Runtime performance visibility
  - `SchedulerStats` type with total tasks, polls, misses
  - Average tasks per wakeup (coalescing efficiency)
  - Current computed period (GCD of task periods)

- [ ] **Energy Profiling**: Measure actual power consumption
  - Per-coalescing-strategy power usage
  - Hardware-specific optimizations
  - Integration with system power APIs

### Performance & Platform Support

- [ ] **no_std Support**: Target embedded systems
  - Remove `std::time` dependency
  - Generic clock abstraction
  - Support for Cortex-M and other embedded platforms
  - Arena allocator to avoid heap allocation

- [ ] **Runtime Abstraction**: Clock and Sleeper traits
  - `Clock` trait for time sources (testing, embedded)
  - `Sleeper` trait for async/blocking sleep strategies
  - Feature flags: `tokio-sleep`, `std-sleep`

- [ ] **SIMD Coalescing**: Vectorize interval sweep
  - For very large task sets (10,000+ tasks)
  - Platform-specific optimizations (ARM NEON, x86 AVX2)

- [ ] **Arena Allocator**: Avoid per-task heap allocation
  - Indexed storage with generation counters
  - Improved cache locality
  - Reduced memory fragmentation

- [ ] **Distributed Scheduling**: Multi-process coordination
  - Shared memory communication
  - Distributed coalescing algorithms
  - Process-level hierarchy

### Research & Verification

- [ ] **Dynamic Priority Adjustment**: Adaptive scheduling
  - User-defined priority functions
  - Adaptive priority based on miss rates
  - Machine learning integration for workload prediction

- [ ] **Configuration Files**: YAML/JSON task definitions
  - Declarative task specification
  - Hot-reload support
  - Schema validation

- [ ] **Formal Verification**: Mathematical correctness proofs
  - TLA+ model of coalescing algorithm
  - Prove no-starvation property with aging enabled
  - Model checking for deadlock freedom

For detailed design and implementation roadmap, see [scheduler-hld.md](scheduler-hld.md).

## License

MIT OR Apache-2.0
