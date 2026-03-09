# Syncopate

A hierarchical, power-aware task scheduler for Rust applications requiring precise timing control.

## Overview

Syncopate provides a flexible scheduler for managing periodic tasks with configurable execution windows. It's designed for applications that need:

- **Deterministic timing**: Schedule tasks to run at specific intervals
- **Execution windows**: Define acceptable time ranges for task execution (early/on-time/late detection)
- **Power efficiency**: Idle durations calculated to minimize CPU wakeups
- **Virtual time**: Simulated clock for deterministic testing
- **Flexible scheduling**: Fixed-rate, fixed-delay, one-shot, and wall-clock-anchored tasks

## Quick Start

Add syncopate to your `Cargo.toml`:

```toml
[dependencies]
syncopate = "0.0.1"
```

## Examples

### Basic Usage

```rust
use std::time::Duration;
use syncopate::{Scheduler, TaskBuilder, Window};

fn main() {
    let mut scheduler = Scheduler::new();

    let task = TaskBuilder::every(Duration::from_secs(1))
        .window(Window::symmetric(Duration::from_millis(50)))
        .name("heartbeat")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();

    loop {
        let Some(sleep_dur) = scheduler.calculate_next_tick() else {
            break;
        };
        std::thread::sleep(sleep_dur);

        let result = scheduler.tick();
        for exec in &result.fired {
            println!("{}: drift={}", exec.task.name.as_deref().unwrap_or("?"), exec.drift);
        }
        for miss in &result.missed {
            println!("{}: {} deadlines missed", miss.task.name.as_deref().unwrap_or("?"), miss.deadlines_missed.len());
        }
    }
}
```

### Deterministic Testing with SimClock

```rust
use std::rc::Rc;
use std::time::Duration;
use syncopate::{Drift, Scheduler, SimClock, TaskBuilder};

let clock = Rc::new(SimClock::new());
let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));

let task = TaskBuilder::every(Duration::from_millis(500))
    .name("test_task")
    .build()
    .unwrap();
scheduler.add_task(task).unwrap();

// Immediate fire at t=0
let result = scheduler.tick();
assert_eq!(result.fired.len(), 1);
assert_eq!(result.fired[0].drift, Drift::OnTime);

// Advance to next deadline
clock.advance(Duration::from_millis(500));
let result = scheduler.tick();
assert_eq!(result.fired.len(), 1);
```

### Wall-Clock-Anchored Tasks

```rust
use std::time::Duration;
use syncopate::{Scheduler, TaskBuilder, Window};

let mut scheduler = Scheduler::new();

// Fires on absolute wall-clock boundaries (e.g. every second at :00, :01, :02...)
let task = TaskBuilder::every_absolute(Duration::from_secs(1))
    .window(Window::symmetric(Duration::from_millis(50)))
    .name("wall_clock_tick")
    .build()
    .unwrap();
scheduler.add_task(task).unwrap();

// With offset: fires at 500ms past each second boundary
let task = TaskBuilder::every_absolute(Duration::from_secs(1))
    .offset(Duration::from_millis(500))
    .window(Window::symmetric(Duration::from_millis(50)))
    .name("half_second_tick")
    .build()
    .unwrap();
scheduler.add_task(task).unwrap();
```

### One-Shot Tasks

```rust
use std::time::Duration;
use syncopate::{Scheduler, TaskBuilder, Window};

let mut scheduler = Scheduler::new();

// Fire once after 5 seconds
let task = TaskBuilder::once_after(Duration::from_secs(5))
    .window(Window::symmetric(Duration::from_millis(50)))
    .name("delayed_action")
    .build()
    .unwrap();
scheduler.add_task(task).unwrap();
```

## Core Concepts

### Task Types

- **Relative** (`TaskBuilder::every`): Period measured from last fire. Supports `FixedRate` (grid-aligned) and `FixedDelay` (drift-accumulating) schedules.
- **Absolute** (`TaskBuilder::every_absolute`): Fires on wall-clock boundaries. Handles clock jumps (suspend/resume) gracefully.
- **One-shot** (`TaskBuilder::once_after`, `TaskBuilder::once_at`): Single-fire tasks that are evicted after execution.

### Execution Windows

Tasks define a `Window` with early and late tolerances:
- **Early**: Task fires before the ideal deadline (within tolerance)
- **On-Time**: Task fires exactly at the deadline
- **Late**: Task fires after the deadline (within tolerance)
- **Missed**: Task could not fire within the window

### Missed Tick Behavior

When a task misses its window (FixedRate only):
- **`Skip`** (default): Report missed deadlines, advance to current position
- **`RunLatest`**: Fire for the most recent deadline, report earlier ones as missed
- **`Burst { max }`**: Fire multiple missed deadlines in rapid succession

### Repeat Control

- `Repeat::Forever` (default): Task runs indefinitely
- `Repeat::Times(n)`: Task fires exactly `n` times, then is evicted

### Timer Delay Compensation

`set_timer_delay()` compensates for consistent OS timer overhead by waking slightly early.

## License

MIT OR Apache-2.0
