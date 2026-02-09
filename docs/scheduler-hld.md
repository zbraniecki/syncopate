# High-Level Design: Syncopate — Two-Tier Hierarchical Task Scheduler for Rust

**Version:** 3.1
**Date:** February 8, 2026
**Status:** Design Phase

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Problem Statement](#problem-statement)
3. [Existing Libraries and Gap Analysis](#existing-libraries-and-gap-analysis)
4. [Goals and Non-Goals](#goals-and-non-goals)
5. [Hardware Constraints and Target Latency](#hardware-constraints-and-target-latency)
6. [Background and Terminology](#background-and-terminology)
7. [Architecture Overview](#architecture-overview)
8. [Core API Design](#core-api-design)
9. [Execution Model: Poll/Plan with Optional Callbacks](#execution-model-pollplan-with-optional-callbacks)
10. [Relative vs Anchored Scheduling](#relative-vs-anchored-scheduling)
11. [Task Lifecycle and Cancellation](#task-lifecycle-and-cancellation)
12. [Sub-Scheduler Constraint Model](#sub-scheduler-constraint-model)
13. [Structured Concurrency and Fault Isolation](#structured-concurrency-and-fault-isolation)
14. [Coalescing Algorithm (Efficiency Tier)](#coalescing-algorithm-efficiency-tier)
15. [Performance Considerations](#performance-considerations)
16. [Implementation Roadmap](#implementation-roadmap)
17. [Future Work](#future-work)
18. [References](#references)
19. [Appendices](#appendices)

---

## Executive Summary

Syncopate is a **hierarchical, power-aware task scheduler** for Rust that combines a **poll/plan execution model** with **optional callback-based dispatch**: the scheduler decides what is due and returns a `PollResult` containing both an idle duration and execution metadata; the application controls sleeping, power management, and — when not using callbacks — task dispatch.

**Key innovations:**

- **Two-tier architecture** sharing core data structures:
  - **Precision tier** — O(log n) EDF peek for sub-millisecond scheduling (10-100 us periods)
  - **Efficiency tier** — O(n log n) weighted interval coalescing for power-saving workloads (1 ms+ periods)
- **Idle-duration-first design** — the primary output is `PollResult { idle_duration, next_wakeup, executed, missed }`, telling the application exactly how long it can sleep or suspend the device
- **Dual execution model** — poll/plan for maximum control, or optional callbacks for ergonomic dispatch. Both work simultaneously on different tasks within the same scheduler
- **Asymmetric timing windows** — per-task `window_before` and `window_after` allow different tolerance for early vs. late execution, a feature absent from all surveyed industry schedulers
- **First-class miss detection** — missed deadlines are reported structurally with consecutive miss counts, not silently swallowed
- **Full task lifecycle** — add, remove, modify, pause, resume tasks at any time. Cooperative cancellation with graceful cleanup
- **Relative and anchored scheduling** — tasks can be relative to creation time (monotonic) or anchored to wall-clock boundaries, with gradual phase alignment that shifts relative tasks toward anchored grid points to maximize idle time
- **Integer nanosecond arithmetic** — periods and windows are `Duration` values; GCD/LCM computed on `u64` nanoseconds with no floating-point comparison bugs
- **Channel-based concurrency** — a cloneable `SchedulerHandle` sends commands; a single-owner `SchedulerLoop` processes them. No shared mutable state in the hot path
- **Runtime-agnostic** — works with Tokio, async-std, smol, bare `std::thread`, or a custom event loop. No dependency on any async runtime

The library draws from EDF scheduling, hierarchical scheduling (Regehr 2001), power-management coalescing, and ergonomic patterns from Tokio, Go, Kotlin coroutines, and Erlang/BEAM to serve use cases from embedded sensor loops to game-engine tick coordination to IoT device power management.

---

## Problem Statement

Modern applications need to coordinate periodic and one-shot tasks across multiple components while:

1. **Minimizing power consumption** by coalescing wakeups and reporting idle duration for device suspend
2. **Respecting timing constraints** with bounded jitter
3. **Maintaining hierarchical boundaries** (components shouldn't dictate global frequency)
4. **Supporting priority differentiation** (critical vs. background tasks)
5. **Handling missed deadlines gracefully** with structured reporting
6. **Remaining runtime-agnostic** (work with Tokio, bare `std::thread`, or a custom event loop)
7. **Reporting idle duration** so the application or OS can enter low-power states between wakeups
8. **Supporting both high-frequency and power-saving workloads** — a single architecture that scales from 10 us sensor reads to hourly batch jobs
9. **Managing dynamic task sets** — tasks are added, removed, paused, and modified throughout the application lifetime
10. **Providing ergonomic APIs** — task creation should be concise, not require 10+ lines of boilerplate

Existing solutions fall short in specific ways, detailed in the next section.

---

## Existing Libraries and Gap Analysis

This section surveys the most significant cooperative scheduling systems across languages, identifies what each focuses on, where they overlap with syncopate's goals, and what gaps syncopate fills.

### Rust Ecosystem

#### Tokio

**Focus:** General-purpose async runtime with work-stealing executor, I/O driver, and timer infrastructure.

**Core abstractions:** `Runtime`, `JoinHandle<T>`, `tokio::time::{sleep, interval, timeout}`, `CancellationToken`, `JoinSet`, `TaskTracker`.

**Scheduling strategy:** M:N work-stealing across a thread pool. Timer wheel (hierarchical hashed timing wheel) for `sleep`/`interval` with ~1ms granularity. No priority system — all tasks are equal.

**Overlap with syncopate:**
- `tokio::time::interval()` provides periodic execution, similar to `TaskType::Periodic`
- `tokio::time::sleep()` provides one-shot delays, similar to `TaskType::OneShot`
- `CancellationToken` provides cooperative cancellation trees

**Gaps that syncopate fills:**
- No timing windows / jitter tolerance — timers fire at exact deadlines or late, with no concept of "acceptable early/late range"
- No idle-duration reporting — the runtime decides when to sleep; the application cannot query "how long until next wakeup?" for device power management
- No priority lanes — all tasks compete equally in the work-stealing queues
- No miss detection — late timers just fire late with no structured reporting
- No coalescing — each timer fires independently, no batching for power savings
- No hierarchical scheduling — flat timer wheel, no sub-scheduler boundaries
- Tightly coupled to the Tokio runtime — cannot be used with bare threads or other runtimes

**Lessons adopted:**
- `JoinHandle<T>` returning a typed result is the gold standard for task handles. Syncopate adopts typed results for one-shot tasks
- `CancellationToken` as a cooperative cancellation tree is excellent. Syncopate adopts hierarchical cancellation through sub-schedulers
- `JoinSet` for managing groups of tasks. Syncopate's sub-schedulers serve a similar grouping role
- `tokio::select!` demonstrates the value of racing concurrent operations

#### async-std

**Focus:** Async runtime mirroring the `std` library API surface, providing familiar abstractions for async code.

**Core abstractions:** `task::spawn()`, `task::sleep()`, `stream::interval()`.

**Scheduling strategy:** Work-stealing executor (originally based on `async-task` / smol). Timer implementation similar to Tokio.

**Overlap with syncopate:** Same overlap as Tokio (periodic intervals, one-shot delays).

**Gaps that syncopate fills:** Same gaps as Tokio — no timing windows, no idle-duration reporting, no priorities, no miss detection, no coalescing, no hierarchy.

**Lessons adopted:**
- Naming conventions that mirror `std` reduce cognitive load. Syncopate follows Rust naming conventions where possible

#### smol

**Focus:** Minimal, composable async runtime in ~1500 lines. Building blocks rather than a monolithic runtime.

**Core abstractions:** `Executor`, `Timer`, `block_on()`. `Timer::after(duration)` and `Timer::at(instant)`.

**Scheduling strategy:** Single-threaded executor with optional work-stealing via `Executor`. Timer based on `async-io` reactor.

**Overlap with syncopate:** `Timer::at(Instant)` for one-shot and `Timer::after(Duration)` for delays. Minimal, composable philosophy.

**Gaps that syncopate fills:** Same core gaps — no timing windows, no idle-duration, no priorities, no miss detection, no coalescing, no hierarchy.

**Lessons adopted:**
- Composability and minimalism. smol proves a focused library can be powerful. Syncopate follows this principle — it schedules; the application executes
- `Timer::at(Instant)` as a clean one-shot API influenced syncopate's `Deadline::Monotonic(Instant)`

### Other Languages

#### Go Runtime Scheduler

**Focus:** M:N goroutine scheduling with integrated network poller and garbage collector coordination.

**Core abstractions:** Goroutine (`go func()`), `context.Context` (deadline, cancellation, key-value propagation), `time.Ticker`, `time.Timer`, `time.AfterFunc()`.

**Scheduling strategy:** Work-stealing with per-P (processor) local run queues and a global queue. Preemptive at cooperative yield points (function calls) since Go 1.14 — added because pure cooperative scheduling caused 10ms+ latency spikes from CPU-bound goroutines. Timer heap per P, checked during scheduling.

**Overlap with syncopate:**
- `time.NewTicker(d)` is periodic scheduling
- `time.AfterFunc(d, f)` is one-shot with callback
- `context.Context` carries deadlines and cancellation through call chains

**Gaps that syncopate fills:**
- No timing windows — tickers fire at fixed intervals or drift
- No idle-duration reporting — Go's runtime manages sleep internally
- No priority system — all goroutines are equal (FIFO with work-stealing)
- No miss detection — missed ticks are either dropped or buffered, not reported
- No coalescing — each ticker is independent
- No hierarchical scheduling — flat scheduler (though scheduling groups exist for NUMA)

**Lessons adopted:**
- **`context.Context` as cancellation tree** is perhaps the most impactful concurrency pattern in the last decade. Syncopate's generic `Ctx` parameter draws from this, though syncopate separates shared state (Ctx) from cancellation (explicit lifecycle methods). A future version may unify these
- **Preemptive scheduling at yield points** — Go discovered that pure cooperative scheduling is insufficient when callbacks can be CPU-heavy. Syncopate documents that callbacks must be fast and provides `TaskExecution` timing data so applications can detect slow callbacks
- **`time.AfterFunc(d, f)`** — one-shot with callback in a single expression. Syncopate adopts similar convenience constructors

#### Kotlin Coroutines

**Focus:** Structured concurrency as the foundation of the coroutine system. All concurrent work is scoped and hierarchically managed.

**Core abstractions:** `CoroutineScope`, `Job`, `Deferred<T>`, `SupervisorJob`, `CoroutineDispatcher`, `withTimeout()`, `withContext()`.

**Scheduling strategy:** Cooperative suspension at `suspend` points. Dispatchers control which thread(s) execute coroutines: `Dispatchers.Default` (shared pool), `Dispatchers.IO` (larger pool for blocking), `Dispatchers.Main` (UI thread), `Dispatchers.Unconfined` (immediate). No priority system within a dispatcher.

**Overlap with syncopate:**
- Hierarchical job trees relate to syncopate's sub-scheduler hierarchy
- Cancellation propagates from parent to children
- `withTimeout()` creates deadline-scoped execution

**Gaps that syncopate fills:**
- No timing windows — timeouts are strict deadlines, no tolerance range
- No idle-duration reporting
- No periodic task scheduling as a first-class concept (must use `while(true) { delay(d) }` loops)
- No miss detection
- No coalescing
- No priority within dispatchers

**Lessons adopted:**
- **Structured concurrency is the defining pattern.** A parent scope owns all child work. Cancelling the parent cancels all children. This influenced syncopate's sub-scheduler lifecycle model where removing a sub-scheduler cancels all its tasks
- **`SupervisorJob`** — allows children to fail independently without cascading to siblings. Syncopate adopts this for sub-scheduler fault isolation
- **`withTimeout()`** — deadline-scoped execution is extremely ergonomic. Syncopate's one-shot tasks with timing windows provide similar functionality at the scheduling level

#### Java Project Loom / Virtual Threads

**Focus:** Lightweight, M:N scheduled threads that look and feel like platform threads but are cheap to create and block.

**Core abstractions:** `Thread.startVirtualThread(Runnable)`, `StructuredTaskScope`, `StructuredTaskScope.ShutdownOnFailure`, `StructuredTaskScope.ShutdownOnSuccess`.

**Scheduling strategy:** Virtual threads are scheduled on a ForkJoinPool using work-stealing. Blocking operations (I/O, locks) unmount the virtual thread from the carrier thread. No priority system for virtual threads.

**Overlap with syncopate:**
- `StructuredTaskScope` provides structured concurrency similar to sub-schedulers

**Gaps that syncopate fills:**
- No periodic scheduling — virtual threads are for general computation, not timed dispatch
- No timing windows, idle-duration, miss detection, coalescing, or hierarchy
- Virtual threads are general-purpose; syncopate is timing-specialized

**Lessons adopted:**
- **`ShutdownOnFailure` / `ShutdownOnSuccess`** — structured task scopes with automatic cancellation on first failure or first success. Syncopate adopts configurable sub-scheduler failure policies
- **Pinning detection** — Loom warns when virtual threads block on monitors (bad for performance). Syncopate should similarly warn when callbacks exceed a configurable time budget

#### Python asyncio

**Focus:** Single-threaded cooperative event loop for I/O-bound async applications.

**Core abstractions:** `EventLoop`, `Task`, `Future`, `TaskGroup` (3.11+), `asyncio.timeout()` (3.11+), `loop.call_later()`, `loop.call_at()`.

**Scheduling strategy:** Single-threaded event loop with a ready queue and a scheduled (timer heap) queue. No priorities — FIFO execution of ready callbacks. `call_later(delay, callback)` and `call_at(when, callback)` for timed execution.

**Overlap with syncopate:**
- `call_later()` / `call_at()` are one-shot scheduling
- `TaskGroup` provides structured concurrency
- Single-threaded event loop with poll-like semantics

**Gaps that syncopate fills:**
- No timing windows — timers fire at or after the deadline, no tolerance range
- No idle-duration as a first-class output — the loop manages sleep internally
- No miss detection — late callbacks just fire late
- No coalescing
- No hierarchy
- Limited to single thread with no Send+Sync considerations

**Lessons adopted:**
- **`call_later(delay, callback)` is the simplest timer API** — one function, two arguments. Syncopate adopts similar convenience constructors to reduce boilerplate
- **`TaskGroup`** (Python 3.11) — structured concurrency where exceptions propagate cleanly. Influenced sub-scheduler error propagation design
- **`asyncio.shield()`** — protects a coroutine from outer cancellation. Syncopate supports "shielded" tasks that survive sub-scheduler cancellation

#### Erlang/BEAM Scheduler

**Focus:** Massively concurrent, fault-tolerant runtime with preemptive scheduling and supervision trees.

**Core abstractions:** Process (lightweight, fully isolated), Supervisor, `gen_server`, `erlang:send_after/3`, `timer` module, Monitor, Link.

**Scheduling strategy:** Preemptive reduction-based scheduling. Each process gets a budget of ~4000 reductions (roughly function calls) before being preempted. Per-scheduler (per-core) run queues with work-stealing between schedulers. Four priority levels: `max`, `high`, `normal`, `low`.

**Overlap with syncopate:**
- `send_after/3` provides delayed one-shot messaging (like one-shot tasks)
- Four priority levels
- Per-core scheduling with work-stealing

**Gaps that syncopate fills:**
- No timing windows — `send_after` fires at exact time or late
- No idle-duration reporting — BEAM manages scheduling internally
- No periodic tasks as first-class (must re-schedule with `send_after` in a loop)
- No coalescing
- No timing-focused hierarchy (BEAM's supervision trees are for fault tolerance, not period budgets)

**Lessons adopted:**
- **Supervision trees** — hierarchical fault management with configurable restart strategies (`one_for_one`, `one_for_all`, `rest_for_one`). Syncopate adopts configurable sub-scheduler failure policies inspired by OTP supervision
- **Reduction-based fairness** — BEAM guarantees fairness even with CPU-heavy processes. Syncopate should document callback time budgets and provide optional slow-callback detection
- **"Let it crash"** philosophy — faults in one process don't bring down the system. Syncopate's sub-scheduler isolation adopts this: a panicking callback in one sub-scheduler doesn't corrupt the parent
- **Priority levels** — BEAM's four levels (`max`, `high`, `normal`, `low`) influenced syncopate's semantic priority classes

### Summary: Where Syncopate Sits

```
                   Timing Precision
                        ^
                        |
              Syncopate |  Erlang/BEAM
              (unique   |  (preemptive,
               niche)   |   fault-tolerant)
                        |
         +--------------+--------------+
         |              |              |
  Power  |   No other   |              |  General
  Aware  |   library    |              |  Purpose
         |   here       |              |
         |              |              |
         +--------------+--------------+
                        |
              asyncio   |  Tokio / Go / smol
              (simple,  |  (work-stealing,
               single-  |   runtime-coupled)
               thread)  |
                        |
                        v
                   General Scheduling
```

**The gap syncopate fills:**

No existing library combines:
1. Timing windows (asymmetric jitter tolerance)
2. Idle-duration-first output (power management)
3. Miss detection (structured deadline violation reporting)
4. Task coalescing (wakeup batching for power savings)
5. Hierarchical scheduling (period-budgeted sub-schedulers)
6. Runtime-agnostic operation (no dependency on any async runtime)
7. Priority lanes with aging (multi-level EDF with starvation prevention)

8. Gradual phase alignment between relative and anchored tasks (novel — no surveyed library offers this)

Each existing library covers at most 1-2 of these. Syncopate covers all eight.

---

## Goals and Non-Goals

### Goals

1. **Dual execution model**: poll/plan for maximum control, optional callbacks for ergonomic dispatch — both work simultaneously
2. **First-class idle duration / next-wakeup API**: the primary value proposition for power management
3. **Two operating tiers**: precision tier for high-frequency dispatch, efficiency tier for power-saving coalescing
4. **Hierarchical scheduling**: support nested sub-schedulers with period constraints and fault isolation
5. **Asymmetric timing windows**: per-task jitter tolerance with independent before/after bounds
6. **Task coalescing**: batch tasks within overlapping windows to minimize wakeups (efficiency tier)
7. **Priority lanes**: multi-level priority queues with EDF within each level, with semantic priority classes
8. **Periodic and one-shot tasks**: support both recurring and alarm-style tasks
9. **Relative and anchored scheduling**: tasks use either monotonic (relative to creation) or wall-clock (anchored to boundaries) time, with gradual phase alignment
10. **First-class miss detection**: missed deadlines reported structurally with consecutive miss counts
11. **Full task lifecycle**: add, remove, modify, pause, resume tasks. Cooperative cancellation
12. **Period negotiation**: sub-schedulers can request parent period changes (opt-in)
13. **Ergonomic task creation**: convenience constructors (including Hz) so common cases require 1-3 lines, not 10+
14. **Runtime abstraction**: `Clock` and `Sleeper` traits for testability and portability; no `spawn` needed
15. **Integer nanosecond arithmetic**: no `f64` Hz in the core API; Hz convenience converts to Duration at the boundary
16. **Fault isolation**: panicking callbacks in sub-schedulers don't corrupt the parent scheduler
17. **Slow-callback detection**: optional time budgets with warnings when callbacks exceed them
18. **First-tick coalescability**: new tasks must be coalescable with existing tasks on their first tick, or be rejected

### Non-Goals

1. **Task execution on threads**: library schedules; application controls execution. No thread pool, no work-stealing executor
2. **Hard real-time guarantees**: soft real-time only — we target bounded jitter on standard and PREEMPT_RT Linux but do not guarantee sub-microsecond deadlines (see [Hardware Constraints](#hardware-constraints-and-target-latency) for why)
3. **Preemption**: tasks run to completion within their callbacks (cooperative scheduling). Unlike BEAM's reduction-based preemption or Go's safepoints, syncopate cannot preempt a running callback
4. **Distributed scheduling**: single-process only (no cross-process coordination)
5. **Automatic priority tuning**: users set priorities explicitly (priority aging is opt-in, not automatic)
6. **I/O multiplexing**: syncopate schedules by time, not by I/O readiness. Use Tokio/mio/polling for I/O
7. **no_std support in initial release**: future goal, not in scope for v1

---

## Hardware Constraints and Target Latency

This section documents the timing characteristics that inform the two-tier architecture. All measurements target the **Raspberry Pi 5** (Cortex-A76, BCM2712) running Linux.

### Timer Resolution

| Source | Resolution | Notes |
|--------|-----------|-------|
| ARM generic timer (`CNTPCT_EL0`) | ~1 us (54 MHz counter) | Direct register read, no syscall |
| `clock_gettime(CLOCK_MONOTONIC)` | ~1 us | vDSO, no kernel transition on ARM64 |
| `std::time::Instant::now()` | ~1 us | Wraps `clock_gettime` on Linux |

### OS Wakeup Latency

| Configuration | Average | P99 | Worst Observed |
|--------------|---------|-----|----------------|
| Standard Linux (6.x, `SCHED_OTHER`) | ~3 us | ~50 us | 75-100 us |
| PREEMPT_RT Linux (`SCHED_FIFO`) | ~10 us | ~30 us | ~80 us |
| Busy-wait (`spin_loop`) | 10-50 ns | ~100 ns | ~200 ns |

**Note:** Busy-wait achieves the best latency but consumes a dedicated core and defeats power management. It is outside the scope of this library.

### Target Scheduling Classes

| Tier | Period Range | Dispatch Complexity | Use Case |
|------|-------------|-------------------|----------|
| **Precision** | 10 us - 1 ms | O(log n) per poll | Sensor fusion, motor control, audio DSP |
| **Efficiency** | 1 ms - hours | O(n log n) per coalesce | IoT telemetry, UI refresh, batch jobs |

### Explicit Limitation

Syncopate is **not suitable for guaranteed sub-1 us scheduling**. The OS kernel's timer and scheduling latency on standard Linux makes this physically impossible without busy-wait or a real-time co-processor. Applications needing nanosecond-precise timing should use dedicated hardware timers or an RTOS.

---

## Background and Terminology

### Computer Science Foundations

#### 1. Real-Time Scheduling

**Earliest Deadline First (EDF)**
- Dynamic priority algorithm: tasks with earliest deadlines run first
- Optimal for single-processor systems when utilization <= 100%
- Handles both periodic and aperiodic (one-shot) tasks
- Our implementation: EDF *within* each priority level

**Rate Monotonic Scheduling (RMS)**
- Fixed-priority algorithm: tasks with shorter periods get higher priority
- Simpler than EDF but less flexible
- Influences our precision tier design

**Jitter-Bounded Scheduling**
- Guarantees maximum deviation from ideal timing
- Our "timing windows" (asymmetric before/after) are explicit jitter bounds

**Slack Time / Least Slack Time (LST)**
- Slack = deadline - remaining execution time - current time
- Our coalescing algorithm uses slack (window size) as the optimization input

#### 2. Hierarchical Scheduling

**Hierarchical Scheduler Infrastructure (HSI)** (Regehr, 2001)
- Tree of schedulers: parent allocates resources to children
- Each level has own policy
- Children cannot exceed parent's budget
- Our design: parent allocates *period budget* instead of CPU time

**Scheduling Domains** (Linux kernel)
- Hierarchical structure for load balancing
- Each level has different rebalancing interval
- Influences our sub-scheduler tree model

#### 3. Task Coalescing

**Definition**: Batching multiple timer expirations into a single wakeup event.

**Power Management Context**
- Modern CPUs/SoCs enter deep sleep states between wakeups
- Wakeup latency from deep sleep can be milliseconds
- Coalescing reduces wakeup count, enabling longer deep sleep intervals
- Idle duration reporting is the key output enabling device-level power management

#### 4. Priority Scheduling

**Multi-Level Queue Scheduling**
- Separate queue per priority level
- Service queues in strict order
- Our design: priority + EDF hybrid (priority first, deadline second)

**Priority Aging / Priority Boosting**
- Increment priority of starving tasks
- Prevents indefinite starvation
- Optional in our design

#### 5. Structured Concurrency (Industry Pattern)

**Origin**: Introduced formally by Martin Sustrik (libdill, 2016), popularized by Kotlin coroutines (2018), adopted by Java Project Loom (2023), Python TaskGroup (3.11, 2022).

**Core principle**: Every concurrent task has a well-defined owner (scope/parent). When the owner is cancelled or completes, all child tasks are cancelled automatically. No orphan tasks.

**Application to syncopate**: Sub-schedulers are the scoping mechanism. Removing a sub-scheduler cancels all its tasks. A parent scheduler's shutdown cascades to all children.

#### 6. Supervision and Fault Isolation (Industry Pattern)

**Origin**: Erlang/OTP supervision trees (1986+).

**Core principle**: A supervisor monitors child processes and applies a restart strategy when they fail. Failure in one child doesn't corrupt siblings or the parent.

**Application to syncopate**: Sub-schedulers provide fault isolation. A panicking callback in a sub-scheduler is caught and reported without corrupting the parent scheduler's state. Configurable failure policies control whether a sub-scheduler continues or stops after callback panics.

### Terminology Mapping

| Our Term | Industry Equivalent |
|----------|---------------------|
| **PollResult** | Schedule output, dispatch plan, tick result |
| **idle_duration** | Sleep budget, suspend window, idle interval |
| **Main Scheduler** | Root scheduler, top-level scheduler |
| **Sub-Scheduler** | Child scheduler, nested scheduler, scheduler domain |
| **Timing Window** | Jitter tolerance, deadline slack, acceptable delay |
| **Task Coalescing** | Timer coalescing, wakeup batching, event aggregation |
| **Period** | Interval, tick rate, scheduling quantum |
| **Precision Tier** | Fast path, low-latency scheduler |
| **Efficiency Tier** | Coalescing path, power-aware scheduler |
| **SchedulerHandle** | Command sender, client handle |
| **SchedulerLoop** | Event loop, scheduler core, processing loop |
| **Failure Policy** | Restart strategy (Erlang), supervision policy |
| **Anchor Grid** | Phase grid, alignment grid, coalescing grid |
| **Phase Alignment** | Timer coalescing, phase correction, drift compensation |
| **Relative Scheduling** | Monotonic scheduling, creation-relative timing |
| **Anchored Scheduling** | Wall-clock scheduling, absolute-phase scheduling |

---

## Architecture Overview

### Two-Tier System Diagram

```
Application
    |
    |  Commands (add/remove/modify/pause/resume tasks)
    v
 SchedulerHandle ---- channel ----> SchedulerLoop
 (Clone + Send)                     (single owner)
    |                                    |
    | atomic reads:                      | processes commands
    |  max_idle_duration()               | computes plans
    |  next_wakeup()                     | invokes callbacks (if present)
    |                                    v
    |                              SchedulerCore
    |                               +-- TaskStorage (indexed arena + generation counters)
    |                               +-- PriorityHeaps (per-level BinaryHeap<Reverse<Instant>>)
    |                               +-- TimeReference (Instant ↔ SystemTime mapping)
    |                               +-- AnchorGrid (GCD of anchored task periods)
    |                               +-- SubScheduler tree
    |                                    |
    |                          +---------+----------+
    |                          v                     v
    |                   Precision Tier        Efficiency Tier
    |                   (EDF peek)            (Coalescing sweep)
    |                   O(log n)              O(n log n)
    |                          |                     |
    |                          +---------+-----------+
    |                                    v
    |                              PollResult {
    |                                idle_duration,
    |                                next_wakeup,
    |                                executed: Vec<ExecutedTask>,
    |                                missed: Vec<MissedTask>,
    |                              }
    |                                    |
    <------------------------------------+
    |
    v
Application:
  1. Sleep for idle_duration (or device suspend)
  2. If not using callbacks: dispatch executed/missed tasks
  3. If using callbacks: tasks already dispatched during poll()
```

### Concurrency Model

**Channel-based command/response:**

- `SchedulerHandle` is `Clone + Send + Sync`. Any thread can submit commands (add task, remove task, modify task, pause, resume, shutdown). Commands are sent via a bounded channel (e.g., `crossbeam::channel`).
- `SchedulerLoop` is single-owner, not `Send`. It processes commands from the channel and computes `PollResult` in a tight loop. No mutex contention in the hot path.
- **Lock-free atomic reads**: `max_idle_duration()` and `next_wakeup()` read from `AtomicU64` values updated by the `SchedulerLoop` after each `poll()`. This allows the application's power-management thread to read idle duration without blocking the scheduler.

**Data flow:**

1. Application thread calls `handle.add_task(config)` -> command enqueued
2. Scheduler loop calls `loop.poll()` -> drains command queue, recomputes schedule
3. If callbacks present: invoked during `poll()` with `TaskExecution` / `TaskMiss` info
4. `PollResult` returned with idle_duration, next_wakeup, and metadata about executed/missed tasks
5. Application sleeps for `poll_result.idle_duration`, then polls again

### Tier Selection

Each scheduler instance operates in one tier, chosen at construction:

- **Precision tier**: uses a min-heap (BinaryHeap) per priority level. `poll()` peeks the heap to find due tasks. No coalescing — tasks fire at their exact deadline (within OS jitter). Best for high-frequency, low-task-count workloads.
- **Efficiency tier**: runs the coalescing sweep algorithm before returning a plan. Tasks are batched into optimal wakeup points within their timing windows. Best for power-sensitive, many-task workloads.

Both tiers share `TaskStorage`, `TaskConfig`, `PollResult`, and all handle/loop infrastructure. The only difference is the `poll()` implementation.

### Internal State: ScheduledTask

Each task in the scheduler's internal storage carries additional scheduling metadata beyond the user-provided `TaskConfig`:

```rust
/// Internal representation of a scheduled task (not part of the public API).
struct ScheduledTask<Ctx> {
    config: TaskConfig<Ctx>,
    state: TaskState,              // Active | Paused | Removed

    /// Next monotonic deadline (used for heap ordering).
    next_deadline: Instant,

    /// For anchored tasks: wall-clock deadline for re-anchoring after clock jumps.
    /// None for relative tasks.
    wall_clock_deadline: Option<SystemTime>,

    /// Natural deadline before alignment shift (for tracking convergence).
    /// For relative tasks being aligned to an anchor grid, this tracks where
    /// the task *would* fire without alignment. The difference between
    /// `unshifted_deadline` and `next_deadline` is the alignment correction.
    unshifted_deadline: Instant,

    /// Accumulated alignment shift in nanoseconds (signed, diagnostic).
    /// Positive = shifted later, negative = shifted earlier.
    /// Used for statistics and debugging convergence behavior.
    alignment_shift_ns: i64,

    /// Generation counter for ABA prevention.
    generation: u32,

    /// Consecutive miss count.
    miss_count: usize,
}
```

### Internal State: SchedulerLoop

The `SchedulerLoop` maintains additional state for time-domain mapping and anchor grid management:

```rust
/// Additional fields in SchedulerLoop (not part of the public API).
struct SchedulerLoopState {
    /// Mapping between monotonic and wall-clock time domains.
    /// Captured at construction, updated after clock jumps.
    time_ref: TimeReference,

    /// The current anchor grid, if any anchored periodic tasks exist.
    /// Recomputed whenever anchored tasks are added or removed.
    anchor_grid: Option<AnchorGrid>,

    /// Threshold for detecting wall-clock jumps.
    /// Default: 100ms. Configurable via SchedulerBuilder.
    clock_jump_threshold: Duration,
}
```

---

## Core API Design

### Key Types

```rust
use std::num::NonZeroU64;
use std::time::{Duration, Instant, SystemTime};

/// Unique task identifier with generation counter.
/// High 32 bits: generation. Low 32 bits: arena index.
/// Prevents ABA problems when tasks are removed and slots reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(NonZeroU64);

/// Unique sub-scheduler identifier with generation counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubSchedulerId(NonZeroU64);
```

### PollResult — The Core Output

```rust
/// The result of a scheduler poll. This is the primary interface between
/// the scheduler and the application.
pub struct PollResult {
    /// How long the application can sleep before the next task is due.
    /// This is the key value for device power management / suspend.
    pub idle_duration: Duration,

    /// The monotonic instant of the next wakeup, if any tasks are scheduled.
    pub next_wakeup: Option<Instant>,

    /// Tasks that were executed during this poll (via callbacks).
    /// Empty if no callbacks are registered.
    pub executed: Vec<ExecutedTask>,

    /// Tasks that are due now but have no callbacks (application must dispatch).
    /// Empty if all tasks have callbacks.
    pub due: Vec<DueTask>,

    /// Tasks that have missed their window entirely.
    pub missed: Vec<MissedTask>,
}

/// A task that was executed via its callback during poll().
pub struct ExecutedTask {
    /// The task's unique identifier.
    pub id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// When the task was actually executed.
    pub actual_time: Instant,

    /// Drift: actual_time - ideal_time.
    pub drift: Duration,

    /// The task's priority level (0 = highest).
    pub priority: u8,
}

/// A task that is due for execution but has no callback (application must dispatch).
pub struct DueTask {
    /// The task's unique identifier.
    pub id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// The task's priority level (0 = highest).
    pub priority: u8,
}

/// A task that missed its execution window.
pub struct MissedTask {
    /// The task's unique identifier.
    pub id: TaskId,

    /// When this task ideally should have run.
    pub ideal_time: Instant,

    /// The end of the acceptable execution window.
    pub window_end: Instant,

    /// Number of consecutive misses for this task.
    pub miss_count: usize,
}
```

### Task Configuration

```rust
/// Configuration for a new task.
///
/// Use convenience constructors for common cases:
/// ```
/// TaskConfig::periodic(Duration::from_millis(100))
///     .window(Duration::from_millis(5), Duration::from_millis(10))
///     .priority(1)
///     .name("sensor_read")
///     .on_execute(|exec, ctx| { /* ... */ })
/// ```
pub struct TaskConfig<Ctx = ()> {
    /// What kind of task (periodic or one-shot).
    pub task_type: TaskType,

    /// Priority level. Use numeric (0 = highest) or semantic class.
    pub priority: u8,

    /// Optional human-readable name for debugging and stats.
    pub name: Option<String>,

    /// Optional callback invoked when the task is executed.
    pub on_execute: Option<TaskCallback<Ctx>>,

    /// Optional callback invoked when the task misses its execution window.
    pub on_miss: Option<MissCallback<Ctx>>,

    /// Optional time budget for the on_execute callback.
    /// If the callback exceeds this duration, a warning is logged.
    /// Does not preempt the callback — purely diagnostic.
    pub callback_budget: Option<Duration>,
}

/// Convenience constructors — reduce boilerplate from 10 lines to 1-3.
///
/// Inspired by Go's `time.NewTicker(d)` and Python's `call_later(d, f)`
/// which are single-expression APIs.
impl<Ctx> TaskConfig<Ctx> {
    /// Create a periodic task with default windows (zero tolerance) and priority 0.
    ///
    /// ```
    /// let config = TaskConfig::periodic(Duration::from_secs(1));
    /// ```
    pub fn periodic(period: Duration) -> Self;

    /// Create a periodic task with symmetric window.
    ///
    /// ```
    /// let config = TaskConfig::periodic_with_window(
    ///     Duration::from_secs(1),
    ///     Duration::from_millis(50),  // +-50ms tolerance
    /// );
    /// ```
    pub fn periodic_with_window(period: Duration, tolerance: Duration) -> Self;

    /// Create a one-shot task firing at a monotonic instant.
    ///
    /// ```
    /// let config = TaskConfig::one_shot(Instant::now() + Duration::from_secs(5));
    /// ```
    pub fn one_shot(deadline: Instant) -> Self;

    /// Create a one-shot task firing after a delay.
    ///
    /// ```
    /// let config = TaskConfig::after(Duration::from_secs(5));
    /// ```
    pub fn after(delay: Duration) -> Self;

    /// Create a periodic task from a frequency in Hz.
    /// Converts to Duration internally: period = 1_000_000_000 / hz nanoseconds.
    /// Panics if hz is 0.
    ///
    /// ```
    /// // 60 FPS game loop (~16.67ms period)
    /// let config = TaskConfig::periodic_hz(60);
    /// ```
    pub fn periodic_hz(hz: u64) -> Self;

    /// Create a periodic task anchored to wall-clock boundaries.
    ///
    /// `anchor` defines phase=0. First deadline = anchor + k*period
    /// where k is the smallest integer making the deadline in the future.
    ///
    /// ```
    /// // Fire every 1s on the exact second boundary:
    /// let config = TaskConfig::periodic_anchored(
    ///     Duration::from_secs(1),
    ///     SystemTime::UNIX_EPOCH,
    /// );
    /// ```
    pub fn periodic_anchored(period: Duration, anchor: SystemTime) -> Self;

    /// Create an anchored periodic task from Hz + wall-clock anchor.
    ///
    /// ```
    /// // 1 Hz anchored to second boundaries:
    /// let config = TaskConfig::periodic_hz_anchored(1, SystemTime::UNIX_EPOCH);
    /// ```
    pub fn periodic_hz_anchored(hz: u64, anchor: SystemTime) -> Self;

    /// Create a one-shot task at a wall-clock time.
    ///
    /// ```
    /// let config = TaskConfig::at_wall_clock(compute_next_2am());
    /// ```
    pub fn at_wall_clock(time: SystemTime) -> Self;

    /// Set asymmetric timing window.
    pub fn window(self, before: Duration, after: Duration) -> Self;

    /// Set priority level (0 = highest).
    pub fn priority(self, priority: u8) -> Self;

    /// Set priority using a semantic class.
    pub fn priority_class(self, class: PriorityClass) -> Self;

    /// Set human-readable name.
    pub fn name(self, name: impl Into<String>) -> Self;

    /// Register an execution callback.
    /// Called synchronously during poll() when the task is due.
    pub fn on_execute<F>(self, f: F) -> Self
    where
        F: Fn(TaskExecution, &Ctx) + Send + Sync + 'static;

    /// Register a miss callback.
    /// Called synchronously during poll() when the task misses its window.
    pub fn on_miss<F>(self, f: F) -> Self
    where
        F: Fn(TaskMiss, &Ctx) + Send + Sync + 'static;

    /// Set a time budget for the execution callback (diagnostic only).
    pub fn callback_budget(self, budget: Duration) -> Self;
}
```

### Task Type

```rust
/// The scheduling pattern for a task.
pub enum TaskType {
    /// Recurring task with a fixed period.
    Periodic {
        /// Time between ideal execution points.
        period: Duration,

        /// Task may execute this much before its ideal time.
        /// Unique to syncopate — no other surveyed library offers
        /// asymmetric jitter tolerance.
        window_before: Duration,

        /// Task may execute this much after its ideal time.
        window_after: Duration,

        /// How the task's phase (offset within its period) is determined.
        /// Default: `PhaseMode::Relative` — phase set by creation time.
        phase: PhaseMode,
    },

    /// One-time task with an absolute deadline.
    OneShot {
        /// When the task should execute.
        /// `Deadline::Monotonic` = relative ("fire in 5 minutes").
        /// `Deadline::WallClock` = anchored ("fire at 12:45pm").
        deadline: Deadline,

        /// Task may execute this much before its deadline.
        window_before: Duration,

        /// Task may execute this much after its deadline.
        window_after: Duration,
    },
}

/// A deadline expressed in either monotonic or wall-clock time.
pub enum Deadline {
    /// Monotonic time (immune to clock adjustments, suitable for most uses).
    /// This is a "relative" deadline — relative to the creation point.
    Monotonic(Instant),

    /// Wall-clock time (for tasks that must fire at a specific real-world time,
    /// e.g., "run at 2:00 AM"). Subject to clock adjustments.
    /// This is an "anchored" deadline — anchored to the wall clock.
    WallClock(SystemTime),
}

/// How a periodic task's phase (offset within its period) is determined.
///
/// This controls the relationship between the task's execution times and
/// the time domain. See Section 10 (Relative vs Anchored Scheduling) for
/// full details on the alignment algorithm.
pub enum PhaseMode {
    /// Phase determined by creation time. Uses monotonic clock.
    /// First deadline = now + period. Subsequent = previous + period.
    ///
    /// The scheduler MAY shift execution within the task's window
    /// to align with an anchor grid, if one exists. This is the
    /// gradual alignment algorithm described in Section 10.
    Relative,

    /// Phase locked to wall-clock boundaries.
    /// Deadlines computed from a wall-clock anchor point:
    ///   anchor + k*period for smallest k making the deadline future.
    ///
    /// Example: `PhaseMode::Anchored { anchor: UNIX_EPOCH }` with
    /// period=1s → fires at :00, :01, :02... on the wall clock.
    Anchored {
        /// Wall-clock reference point defining phase=0.
        /// Common choices:
        ///   - `SystemTime::UNIX_EPOCH` — align to absolute clock boundaries
        ///   - A specific wall-clock time — align to that reference
        anchor: SystemTime,
    },
}
```

### Priority System

```rust
/// Semantic priority classes, inspired by Erlang/BEAM's four priority levels
/// and Kepler's UI-aware priority model.
///
/// Each class maps to a numeric range. Within a class, numeric priority
/// provides fine-grained ordering.
pub enum PriorityClass {
    /// Critical system tasks (0). Sensor fusion, safety-critical.
    /// Equivalent to BEAM's `max` or Kepler's `UI_CRITICAL`.
    Critical,

    /// User-initiated tasks (1). Responses to user actions.
    /// Equivalent to Kepler's `USER_INITIATED`.
    High,

    /// Normal tasks (2). Default for most periodic work.
    /// Equivalent to BEAM's `normal`.
    Normal,

    /// Background tasks (3). Telemetry, batch processing.
    /// Equivalent to Kepler's `BACKGROUND`.
    Background,

    /// Low-priority tasks (4). Housekeeping, optional cleanup.
    /// Equivalent to BEAM's `low`.
    Low,
}

impl PriorityClass {
    /// Convert to numeric priority.
    pub fn as_u8(&self) -> u8;
}

/// Priority aging policy to prevent starvation.
///
/// Inspired by BEAM's reduction-based fairness and classical
/// multi-level feedback queue aging.
pub struct AgingPolicy {
    /// How long before a task is considered starving.
    pub starvation_threshold: Duration,

    /// How much to boost priority (e.g., +1 level).
    pub priority_boost: u8,
}
```

### Task Modification

```rust
/// A patch to apply to an existing task. All fields are optional;
/// only `Some` values are applied.
///
/// Inspired by Kepler's ability to modify timer parameters at runtime
/// and Go's ability to reset a Ticker's period.
pub struct TaskPatch {
    pub priority: Option<u8>,
    pub period: Option<Duration>,
    pub window_before: Option<Duration>,
    pub window_after: Option<Duration>,
    pub name: Option<Option<String>>,
}
```

### Sub-Scheduler Configuration

```rust
/// Configuration for a sub-scheduler.
/// Period-based, not frequency-based. No integer-divisor constraint.
pub struct SubSchedulerConfig {
    /// The sub-scheduler's tick period. Must be >= parent's period.
    pub period: Duration,

    /// Whether this sub-scheduler can request period changes from its parent.
    pub allow_negotiation: bool,

    /// Failure policy when a callback in this sub-scheduler panics.
    /// Inspired by Erlang/OTP supervision strategies.
    pub failure_policy: FailurePolicy,
}

/// What happens when a callback panics within a sub-scheduler.
///
/// Inspired by Erlang/OTP's `one_for_one`, `one_for_all`, and
/// `rest_for_one` supervision strategies, adapted for task scheduling.
pub enum FailurePolicy {
    /// Log the panic and continue scheduling remaining tasks.
    /// The panicking task is marked as failed but stays registered.
    /// Equivalent to Erlang's `one_for_one` — isolate the failure.
    Continue,

    /// Stop the entire sub-scheduler after a panic.
    /// All tasks in this sub-scheduler are paused.
    /// Parent receives a `SubSchedulerFailed` event.
    Stop,

    /// Stop and restart the sub-scheduler after a panic.
    /// All tasks are rescheduled from their initial state.
    Restart {
        /// Maximum restart attempts within the window.
        max_restarts: u32,
        /// Time window for counting restarts.
        within: Duration,
    },
}
```

### Scheduler Handle (Command Sender)

```rust
/// Cloneable, Send + Sync handle for submitting commands to the scheduler.
/// All mutating methods enqueue a command and return immediately.
///
/// Inspired by Tokio's split between `Runtime` (owns the event loop) and
/// `Handle` (cloneable reference for spawning), and by the actor model
/// pattern of sending messages to a single-owner processor.
impl<Ctx> SchedulerHandle<Ctx>
where
    Ctx: Send + Sync + 'static,
{
    /// Add a new task. Returns a TaskId for future reference.
    pub fn add_task(&self, config: TaskConfig<Ctx>) -> Result<TaskId, SchedulerError>;

    /// Remove a task by ID. The task is unscheduled immediately.
    /// Periodic tasks stop recurring. One-shot tasks are cancelled.
    pub fn remove_task(&self, id: TaskId) -> Result<(), SchedulerError>;

    /// Modify a task's configuration.
    /// Inspired by Go's `Ticker.Reset()` for changing period at runtime.
    pub fn modify_task(&self, id: TaskId, patch: TaskPatch) -> Result<(), SchedulerError>;

    /// Pause a task (stops scheduling it without removing it).
    pub fn pause_task(&self, id: TaskId) -> Result<(), SchedulerError>;

    /// Resume a paused task.
    pub fn resume_task(&self, id: TaskId) -> Result<(), SchedulerError>;

    /// Spawn a child sub-scheduler.
    ///
    /// The sub-scheduler provides hierarchical grouping and fault isolation.
    /// Removing the sub-scheduler cancels all its tasks (structured concurrency,
    /// inspired by Kotlin's CoroutineScope and Java's StructuredTaskScope).
    pub fn spawn_sub_scheduler(
        &self,
        config: SubSchedulerConfig,
    ) -> Result<SubSchedulerHandle<Ctx>, SchedulerError>;

    /// Remove a sub-scheduler and all its tasks.
    /// This is the structured concurrency "scope exit" — all child tasks
    /// are cancelled, inspired by Kotlin's `coroutineScope` block completion.
    pub fn remove_sub_scheduler(&self, id: SubSchedulerId) -> Result<(), SchedulerError>;

    /// Read the maximum idle duration (time until next wakeup).
    /// Lock-free atomic read — safe to call from any thread at any time.
    pub fn max_idle_duration(&self) -> Duration;

    /// Read the next wakeup instant, if any tasks are scheduled.
    /// Lock-free atomic read.
    pub fn next_wakeup(&self) -> Option<Instant>;

    /// Get scheduler statistics.
    pub fn stats(&self) -> SchedulerStats;

    /// Initiate graceful shutdown: drain pending commands, cancel all tasks,
    /// cascade shutdown to all sub-schedulers.
    pub fn shutdown(&self);
}
```

### Scheduler Loop (Event Processor)

```rust
/// Single-owner scheduler loop. Processes commands and computes plans.
/// Not Send — must be owned by one thread/task.
impl<Ctx> SchedulerLoop<Ctx> {
    /// Drain pending commands, advance time, execute callbacks, compute the next plan.
    ///
    /// This is a synchronous, non-blocking call. The application controls
    /// when and how often to poll.
    ///
    /// If tasks have callbacks: callbacks are invoked during this call,
    /// and executed tasks appear in `poll_result.executed`.
    ///
    /// If tasks have no callbacks: due tasks appear in `poll_result.due`
    /// for the application to dispatch.
    ///
    /// Missed tasks always appear in `poll_result.missed` regardless of
    /// whether they have callbacks.
    pub fn poll(&mut self) -> PollResult;

    /// Add a task directly without going through the channel.
    /// For single-threaded usage where SchedulerHandle is not needed.
    pub fn add_task_local(&mut self, config: TaskConfig<Ctx>) -> Result<TaskId, SchedulerError>;

    /// Remove a task directly (single-threaded mode).
    pub fn remove_task_local(&mut self, id: TaskId) -> Result<(), SchedulerError>;
}
```

### Builder

```rust
/// Scheduler tier selection.
pub enum SchedulerTier {
    /// O(log n) dispatch, no coalescing. For high-frequency workloads.
    Precision,

    /// O(n log n) coalescing sweep. For power-saving workloads.
    Efficiency,
}

/// Builder for constructing a scheduler.
///
/// Convenience: common configurations require only 1-2 method calls.
/// Full customization available for advanced use cases.
pub struct SchedulerBuilder<Ctx = ()> { /* ... */ }

impl SchedulerBuilder<()> {
    pub fn new() -> Self;
}

impl<Ctx> SchedulerBuilder<Ctx> {
    /// Set the scheduler tier (precision or efficiency). Default: Efficiency.
    pub fn tier(self, tier: SchedulerTier) -> Self;

    /// Set the minimum period (maximum frequency). Default: 1 ms.
    pub fn min_period(self, period: Duration) -> Self;

    /// Set the maximum period (minimum frequency). Default: 1 hour.
    pub fn max_period(self, period: Duration) -> Self;

    /// Number of priority levels. Default: 5 (matching PriorityClass).
    pub fn priority_levels(self, levels: usize) -> Self;

    /// Enable priority aging to prevent starvation.
    pub fn enable_priority_aging(self, policy: AgingPolicy) -> Self;

    /// Allow sub-schedulers to negotiate period changes.
    pub fn allow_negotiation(self, allow: bool) -> Self;

    /// Set the clock-jump detection threshold. Default: 100ms.
    /// If the wall clock jumps by more than this amount between polls,
    /// the scheduler re-anchors all anchored tasks.
    pub fn clock_jump_threshold(self, threshold: Duration) -> Self;

    /// Set shared context accessible to all callbacks.
    ///
    /// Inspired by Go's `context.Context` pattern for propagating
    /// shared state through a call chain, but type-safe and without
    /// requiring `interface{}` / `any` type assertions.
    pub fn with_context<NewCtx>(self, context: NewCtx) -> SchedulerBuilder<NewCtx>;

    /// Build the scheduler, returning a (handle, loop) pair.
    /// Requires `Ctx: Send + Sync + 'static` for the channel.
    pub fn build(self) -> (SchedulerHandle<Ctx>, SchedulerLoop<Ctx>)
    where
        Ctx: Send + Sync + 'static;

    /// Build in single-threaded mode, returning only the loop.
    /// No channel overhead. No Send + Sync requirement on Ctx.
    ///
    /// Useful for embedded systems or integration into an existing
    /// event loop (e.g., a game engine's main loop).
    pub fn build_local(self) -> SchedulerLoop<Ctx>;

    /// Build with a custom clock (for testing or embedded use).
    pub fn build_with_clock<C: Clock>(self, clock: C) -> (SchedulerHandle<Ctx>, SchedulerLoop<Ctx>)
    where
        Ctx: Send + Sync + 'static;
}
```

### Runtime Abstraction — Clock and Sleeper Only

```rust
/// Abstraction over time sources.
///
/// Enables deterministic testing with mock clocks — a feature missing
/// from the current implementation that every serious scheduler needs.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

/// Default clock using std::time::Instant.
pub struct StdClock;
impl Clock for StdClock {
    fn now(&self) -> Instant { Instant::now() }
}

/// Mock clock for deterministic testing.
/// Inspired by Tokio's `tokio::time::pause()` / `advance()` and
/// Go's `testing` time control.
pub struct MockClock { /* ... */ }
impl MockClock {
    pub fn new(start: Instant) -> Self;
    pub fn advance(&self, duration: Duration);
    pub fn set(&self, instant: Instant);
}

/// Abstraction over sleeping/waiting.
/// The application chooses how to sleep — the scheduler never sleeps internally.
pub trait Sleeper {
    /// Block or async-wait until the given deadline.
    fn sleep_until(&self, deadline: Instant);
}
```

No `spawn` trait needed. The scheduler never executes tasks or spawns futures. The application uses whatever concurrency model it prefers.

### Statistics

```rust
/// Scheduler statistics.
///
/// Inspired by Erlang's `:scheduler.utilization()` and
/// Go's `runtime.ReadMemStats()`.
pub struct SchedulerStats {
    /// Total tasks registered (including paused).
    pub total_tasks: usize,

    /// Tasks currently active (not paused).
    pub active_tasks: usize,

    /// Tasks currently paused.
    pub paused_tasks: usize,

    /// Total number of poll() calls.
    pub total_polls: u64,

    /// Total executed tasks (via callback).
    pub total_executed: u64,

    /// Total missed deadlines.
    pub total_misses: u64,

    /// Average tasks per wakeup (coalescing efficiency, efficiency tier only).
    pub avg_tasks_per_wakeup: f64,

    /// Number of callbacks that exceeded their time budget.
    pub slow_callbacks: u64,

    /// Current computed global period (GCD of all task periods).
    pub current_period: Duration,

    /// Number of sub-schedulers.
    pub sub_scheduler_count: usize,
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("task not found: {0:?}")]
    TaskNotFound(TaskId),

    #[error("task already paused: {0:?}")]
    AlreadyPaused(TaskId),

    #[error("task not paused: {0:?}")]
    NotPaused(TaskId),

    #[error("invalid priority: {0} (must be < {1})")]
    InvalidPriority(u8, usize),

    #[error("period {0:?} is outside bounds [{1:?}, {2:?}]")]
    PeriodOutOfBounds(Duration, Duration, Duration),

    #[error("sub-scheduler period {child:?} must be >= parent period {parent:?}")]
    ChildPeriodTooShort { child: Duration, parent: Duration },

    #[error("sub-scheduler not found: {0:?}")]
    SubSchedulerNotFound(SubSchedulerId),

    #[error("scheduler is shut down")]
    ShutDown,

    #[error("command channel full")]
    ChannelFull,

    #[error("task not coalescable: first tick at {first_tick:?}, nearest existing window {distance:?} away")]
    NotCoalescable {
        /// The monotonic instant of the new task's first tick.
        first_tick: Instant,
        /// Distance from the first tick to the nearest existing task's window edge.
        /// The application can use this to decide how much to widen the window.
        distance: Duration,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum NegotiationError {
    #[error("period negotiation not allowed by parent policy")]
    NotAllowed,

    #[error("requested period {0:?} is shorter than parent minimum")]
    ExceedsParentCapacity(Duration),

    #[error("requested change would cause deadline misses")]
    WouldCauseMisses,
}
```

---

## Execution Model: Poll/Plan with Optional Callbacks

Syncopate supports two execution models simultaneously. This dual approach is informed by the strengths and weaknesses observed across industry libraries.

### Model 1: Poll/Plan (Maximum Control)

The application polls the scheduler, receives a `PollResult`, and dispatches tasks itself. This is the approach used by game engines, embedded systems, and applications that need fine-grained control over execution order and timing.

```rust
let (handle, mut scheduler) = SchedulerBuilder::new()
    .tier(SchedulerTier::Precision)
    .build();

// No callbacks — tasks appear in poll_result.due
let sensor_id = handle.add_task(
    TaskConfig::periodic(Duration::from_millis(100))
        .window(Duration::from_millis(5), Duration::from_millis(10))
        .name("sensor_read")
).unwrap();

loop {
    let result = scheduler.poll();

    // Application dispatches tasks
    for task in &result.due {
        match task.id {
            id if id == sensor_id => read_sensor(),
            _ => {}
        }
    }

    // Handle misses
    for miss in &result.missed {
        log::warn!("Missed {:?}, {} consecutive", miss.id, miss.miss_count);
    }

    std::thread::sleep(result.idle_duration);
}
```

**When to use:** Game loops, embedded systems, code that must control the exact execution order, integration with existing dispatch mechanisms.

**Industry precedent:** This is closest to the original design and is unique to syncopate. No surveyed library returns a plan for the application to dispatch.

### Model 2: Callbacks (Ergonomic Dispatch)

Tasks carry callbacks that are invoked during `poll()`. The application only needs to sleep for the idle duration. This reduces boilerplate and is the recommended approach for most use cases.

```rust
let (handle, mut scheduler) = SchedulerBuilder::new()
    .with_context(AppContext::new())
    .build();

// Callbacks registered at task creation — ergonomic, 3-line setup
handle.add_task(
    TaskConfig::periodic(Duration::from_millis(100))
        .window(Duration::from_millis(5), Duration::from_millis(10))
        .name("sensor_read")
        .on_execute(|exec, ctx| {
            ctx.read_sensor();
            if exec.drift > Duration::from_millis(20) {
                log::warn!("High drift: {:?}", exec.drift);
            }
        })
        .on_miss(|miss, ctx| {
            ctx.record_miss(miss.task_id, miss.miss_count);
        })
).unwrap();

loop {
    let result = scheduler.poll();  // Callbacks invoked here
    std::thread::sleep(result.idle_duration);
}
```

**When to use:** Application code, IoT devices, monitoring systems — any case where callbacks are natural and execution order within a poll cycle doesn't matter.

**Industry precedent:** Go's `time.AfterFunc()`, Python's `call_later()`, Kepler's `IFunction*` parameter. The callback model is the dominant pattern in timer APIs.

### Mixed Mode

Both models work simultaneously. Tasks with callbacks are dispatched automatically; tasks without callbacks appear in `poll_result.due`. This allows gradual migration and mixing of execution models.

### Callback Contract

Callbacks execute synchronously during `poll()`. This is a deliberate design choice:

1. **No hidden concurrency** — the application knows exactly when callbacks run (during `poll()`)
2. **No Send+Sync requirement for local mode** — `build_local()` allows `Rc<RefCell<T>>` in callbacks
3. **Predictable ordering** — callbacks fire in priority order, then deadline order

**Performance warning** (lesson from Go 1.14): Callbacks must be fast. A slow callback blocks the entire `poll()` cycle, which can cause deadline misses for other tasks. For CPU-heavy or I/O-bound work, spawn an async task from within the callback:

```rust
.on_execute(|exec, ctx| {
    // Fast: enqueue work to a channel or spawn an async task
    ctx.work_sender.send(WorkItem::SensorRead(exec.task_id)).ok();
    // Don't: do_expensive_computation() — blocks the scheduler
})
```

The optional `callback_budget` field enables diagnostic detection of slow callbacks (inspired by Java Loom's pinning detection for virtual threads).

---

## Relative vs Anchored Scheduling

Syncopate supports two scheduling modes — **relative** and **anchored** — that coexist within the same scheduler. This is a novel design: no surveyed library offers both modes with gradual phase alignment between them.

### Concept

| Mode | Periodic | One-Shot |
|------|----------|----------|
| **Relative** | "fire every 1s from NOW" — `PhaseMode::Relative` | "fire in 5 minutes" — `Deadline::Monotonic` |
| **Anchored** | "fire every 1s on the second" — `PhaseMode::Anchored` | "fire at 12:45pm" — `Deadline::WallClock` |

**Relative tasks** use monotonic time (`Instant`). They are immune to clock adjustments. Phase is determined by creation time: first deadline = `now + period`, subsequent = `previous + period`. The scheduler MAY shift execution within the task's window to align with an anchor grid (see Gradual Alignment below).

**Anchored tasks** use wall-clock time (`SystemTime`) for phase computation, converted to monotonic time for internal scheduling. They are subject to clock adjustments — the scheduler re-anchors after detected clock jumps. Phase is locked to wall-clock boundaries: `anchor + k*period` for the smallest `k` that places the deadline in the future.

### Example

```rust
// Relative: "poll sensors every 100ms starting now"
let sensor = TaskConfig::periodic(Duration::from_millis(100))
    .window(Duration::from_millis(5), Duration::from_millis(10))
    .name("sensor_read");

// Anchored: "fire every 1s on the exact second boundary"
let heartbeat = TaskConfig::periodic_anchored(
    Duration::from_secs(1),
    SystemTime::UNIX_EPOCH,
)
    .window(Duration::from_millis(50), Duration::from_millis(50))
    .name("heartbeat");

// Relative one-shot: "fire in 5 minutes"
let reminder = TaskConfig::after(Duration::from_secs(300))
    .name("reminder");

// Anchored one-shot: "fire at 2:00 AM"
let backup = TaskConfig::at_wall_clock(next_2am())
    .name("backup");
```

### TimeReference — Monotonic ↔ Wall-Clock Mapping

The scheduler captures a `(Instant, SystemTime)` pair at construction to establish a mapping between the two time domains:

```rust
/// Mapping between monotonic and wall-clock time.
/// Captured at scheduler construction, updated after clock jumps.
pub struct TimeReference {
    /// A monotonic reference point.
    mono_ref: Instant,
    /// The corresponding wall-clock reference point.
    wall_ref: SystemTime,
}

impl TimeReference {
    /// Convert a wall-clock time to a monotonic instant.
    /// Uses duration arithmetic from the reference pair.
    pub fn wall_to_mono(&self, wall: SystemTime) -> Instant {
        match wall.duration_since(self.wall_ref) {
            Ok(d) => self.mono_ref + d,
            Err(e) => self.mono_ref - e.duration(),
        }
    }

    /// Convert a monotonic instant to a wall-clock time.
    pub fn mono_to_wall(&self, mono: Instant) -> SystemTime {
        if mono >= self.mono_ref {
            self.wall_ref + (mono - self.mono_ref)
        } else {
            self.wall_ref - (self.mono_ref - mono)
        }
    }
}
```

The `TimeReference` is initialized in `SchedulerBuilder::build()`:

```rust
let time_ref = TimeReference {
    mono_ref: Instant::now(),
    wall_ref: SystemTime::now(),
};
```

### Anchor Grid

When one or more anchored periodic tasks exist, they define an **anchor grid** — a set of ideal execution times that the scheduler prefers for wakeups.

```rust
/// The anchor grid defines preferred wakeup times derived from
/// anchored periodic tasks.
pub struct AnchorGrid {
    /// GCD of all anchored task periods.
    /// This is the finest granularity at which grid points occur.
    period: Duration,

    /// A wall-clock reference point on the grid.
    /// Typically the anchor of the first anchored task added.
    reference: SystemTime,
}
```

**Grid computation:**
- The grid period is the GCD of all active anchored task periods (computed on `u64` nanoseconds, same as the global period computation)
- Grid points are: `reference + k * period` for all integer `k`
- The grid is **recomputed** whenever an anchored task is added or removed

**Example:** Two anchored tasks with periods 1s and 500ms (both anchored to `UNIX_EPOCH`):
- GCD(1s, 500ms) = 500ms
- Grid points: ..., :00.000, :00.500, :01.000, :01.500, ...

### Gradual Alignment Algorithm

When an anchor grid exists, the scheduler gradually shifts **relative** tasks toward grid points. This maximizes idle time by aligning wakeups across both relative and anchored tasks.

**Algorithm (executed after each relative task fires):**

```
1. Compute the natural next deadline:
     unshifted_deadline = previous_deadline + period

2. Find the nearest anchor grid point to unshifted_deadline:
     grid_point = nearest grid point to unshifted_deadline

3. Compute the distance and direction:
     distance = grid_point - unshifted_deadline   (signed)

4. Compute the maximum allowed shift:
     if distance < 0:  // grid point is earlier
       max_shift = min(|distance|, window_before)
     else:             // grid point is later
       max_shift = min(|distance|, window_after)

5. Apply the shift:
     next_deadline = unshifted_deadline + clamp(distance, -window_before, +window_after)

6. Record diagnostics:
     alignment_shift_ns += (next_deadline - unshifted_deadline).as_nanos()
```

**Visual example:**

```
Period = 1s, window = ±50ms, anchor grid period = 1s (on the second)

Task created at t=0.300s (300ms after the second):

Cycle 1: natural=1.300s, grid=1.000s, distance=-300ms, shift=-50ms → fires at 1.250s
Cycle 2: natural=2.250s, grid=2.000s, distance=-250ms, shift=-50ms → fires at 2.200s
Cycle 3: natural=3.200s, grid=3.000s, distance=-200ms, shift=-50ms → fires at 3.150s
Cycle 4: natural=4.150s, grid=4.000s, distance=-150ms, shift=-50ms → fires at 4.100s
Cycle 5: natural=5.100s, grid=5.000s, distance=-100ms, shift=-50ms → fires at 5.050s
Cycle 6: natural=6.050s, grid=6.000s, distance= -50ms, shift=-50ms → fires at 6.000s ✓ ALIGNED
Cycle 7: natural=7.000s, grid=7.000s, distance=   0ms, shift=  0ms → fires at 7.000s ✓ STABLE
```

**Convergence properties:**

- A task with symmetric window `W` converges in at most `ceil(initial_offset / W)` cycles
- Example: 300ms offset, 50ms window → converges in 6 cycles (6 seconds)
- General formula: `ceil(grid_period / (2 * W))` cycles for worst-case offset
- Example: 1s grid, 50ms window → worst case ~10 cycles (10 seconds)

**Once aligned:** The task stays locked to the grid. Each cycle, `unshifted_deadline` equals the grid point (because the previous `next_deadline` was on the grid), so `distance = 0` and no shift is applied.

**Grid removed:** If the last anchored task is removed, the anchor grid is destroyed. Relative tasks that were aligned will gradually revert to their natural phase as their `unshifted_deadline` drifts away from the now-absent grid. In practice, they'll stay approximately where they are since `unshifted_deadline` tracks the shifted deadline.

### First-Tick Validation (Task Rejection)

When adding a new periodic task, the scheduler validates that the task's first-tick execution window overlaps with at least one existing task's execution window at that time. This ensures the new task can share a wakeup point on its very first execution — it's coalescable from the start.

**Rule:**

1. Compute the new task's first execution window: `[first_deadline - window_before, first_deadline + window_after]`
2. For each existing active task, compute its execution window at the time of the new task's first tick (by projecting forward from the existing task's next deadline)
3. Check if any existing task's projected window overlaps with the new task's first window
4. If **no overlap** and the scheduler has existing tasks → reject with `SchedulerError::NotCoalescable`

**Exceptions (always accepted):**

- The scheduler has no existing tasks (first task is always accepted)
- The new task is a one-shot task (it fires once and is removed; the overhead of one extra wakeup is acceptable)
- The new task is added to an empty sub-scheduler

**Rationale:** This prevents tasks that would force extra wakeup points, defeating the power-management goal. The application can widen the window and retry:

```rust
let result = handle.add_task(
    TaskConfig::periodic(Duration::from_secs(1))
        .window(Duration::ZERO, Duration::ZERO)  // zero tolerance
        .name("strict_task")
);

match result {
    Err(SchedulerError::NotCoalescable { distance, .. }) => {
        // Widen the window to at least `distance` and retry
        handle.add_task(
            TaskConfig::periodic(Duration::from_secs(1))
                .window(distance, distance)
                .name("strict_task")
        ).unwrap();
    }
    Ok(id) => { /* accepted */ }
    Err(e) => { /* other error */ }
}
```

**Projection algorithm for existing tasks:**

For a periodic task with period `P`, next deadline `D`, and windows `[wb, wa]`:
- Compute the tick count at the new task's first deadline `T`: `k = ceil((T - D) / P)`
- Projected deadline: `D + k * P`
- Projected window: `[D + k*P - wb, D + k*P + wa]`
- Check overlap: `new_start <= projected_end && projected_start <= new_end`

### Clock Jump Detection and Re-Anchoring

Each `poll()` call checks for wall-clock jumps by comparing the expected wall-clock time against the actual:

```
expected_wall = time_ref.mono_to_wall(Instant::now())
actual_wall = SystemTime::now()
jump = |expected_wall - actual_wall|

if jump > clock_jump_threshold:
    // Clock jumped — re-anchor
    1. Update TimeReference with new (Instant::now(), SystemTime::now()) pair
    2. For each anchored task:
       - Recompute next_deadline from wall_clock_deadline using new TimeReference
    3. Rebuild the priority heap
    4. Recompute the anchor grid
    5. Log warning: "clock jump detected: {jump:?}"
```

**Key property:** Relative tasks are completely unaffected by clock jumps. They use monotonic time (`Instant`), which by definition never jumps. Only anchored tasks need re-anchoring.

**Common clock jump causes:**
- NTP synchronization (typically < 1s, but can be large on first sync)
- Manual time adjustment
- VM migration or suspend/resume
- Leap seconds (rare, 1s jump)

### Idle Duration with Grid Preference

When computing idle duration, the scheduler considers the anchor grid:

```
next_task_window_start = earliest start of any task's next window
next_grid_point = nearest future anchor grid point

if anchor_grid exists AND next_grid_point < next_task_window_start:
    // Wake at the grid point to accelerate alignment of relative tasks
    idle_duration = next_grid_point - now
else:
    // No grid, or grid point is after next task — wake at task window
    idle_duration = next_task_window_start - now
```

This accelerates the convergence of the gradual alignment algorithm by ensuring the scheduler wakes at grid points even when no task is strictly due. The cost is one extra wakeup per grid period during the convergence phase; once all relative tasks are aligned, grid points coincide with task deadlines and no extra wakeups occur.

---

## Task Lifecycle and Cancellation

Every surveyed library (Tokio, Go, Kotlin, Java Loom, Python, Erlang) supports task cancellation. This section defines syncopate's task lifecycle, informed by industry patterns.

### State Machine

```
                add_task()
                    |
                    v
    +--------->  Active  <---------+
    |               |              |
    |     pause()   |   remove()   |
    |               v              |
    |           Paused             |
    |               |              |
    |    resume()   |   remove()   |
    |               v              |
    +----------  Active  ------>  Removed
                                   |
                                   v
                               (slot freed,
                                generation
                                incremented)
```

### Task States

- **Active**: Task is scheduled and will appear in `PollResult` when due
- **Paused**: Task is registered but excluded from scheduling. Its deadline doesn't advance. Resuming restarts from where it left off
- **Removed**: Task is unregistered. Its arena slot is freed and the generation counter is incremented to prevent ABA

### Lifecycle Methods

```rust
// Add a task (returns Active)
let id = handle.add_task(config)?;

// Pause — task stays registered but won't fire
handle.pause_task(id)?;

// Resume — task resumes scheduling from where it paused
handle.resume_task(id)?;

// Modify — change period, priority, windows while active or paused
handle.modify_task(id, TaskPatch {
    period: Some(Duration::from_millis(200)),
    ..Default::default()
})?;

// Remove — permanently cancel the task
handle.remove_task(id)?;

// Using a removed TaskId returns Err(TaskNotFound)
assert!(handle.pause_task(id).is_err());
```

### Cancellation via Sub-Schedulers (Structured Concurrency)

Removing a sub-scheduler cancels all its tasks. This provides structured concurrency: when a component is done, all its scheduled work is cleaned up automatically.

```rust
let physics = handle.spawn_sub_scheduler(SubSchedulerConfig {
    period: Duration::from_millis(200),
    allow_negotiation: false,
    failure_policy: FailurePolicy::Continue,
})?;

physics.add_task(TaskConfig::periodic(Duration::from_millis(200)).name("step"))?;
physics.add_task(TaskConfig::periodic(Duration::from_secs(1)).name("debug"))?;

// Later: remove the sub-scheduler — both tasks are cancelled
handle.remove_sub_scheduler(physics.id())?;
```

This is equivalent to Kotlin's `coroutineScope { }` completing — all child coroutines are cancelled when the scope exits. It ensures no orphan tasks persist after a component shuts down.

---

## Sub-Scheduler Constraint Model

### Relaxed Period Constraint

The original design required child frequency to be an integer divisor of the parent frequency. This was too restrictive and introduced floating-point comparison bugs when using `f64` Hz values.

**New rule: a child sub-scheduler's period must be >= the parent's period. That's the only constraint.**

Rationale:
- The coalescing engine (efficiency tier) handles alignment naturally via timing windows. There is no need for tasks to land on a fixed frequency grid.
- The precision tier fires tasks at their exact deadlines regardless of parent/child relationships.
- The integer-divisor constraint prevented useful configurations like parent=100ms, child=150ms (ratio 1.5).
- GCD/LCM of periods is still computed (on `u64` nanoseconds) for global period optimization, but it's an internal optimization, not an external constraint.

### What This Means in Practice

```
Parent period: 100ms
Valid child periods: 100ms, 150ms, 200ms, 500ms, 1s, ...
Invalid child periods: 50ms (< parent), 10ms (< parent)
```

The `FrequencyNotDivisor` error from v1 is removed. The only sub-scheduler error is `ChildPeriodTooShort`.

---

## Structured Concurrency and Fault Isolation

This section describes how sub-schedulers provide structured concurrency (lifetime management) and fault isolation (panic handling), drawing on patterns from Kotlin coroutines, Java Loom's `StructuredTaskScope`, and Erlang/OTP supervision trees.

### Structured Concurrency via Sub-Schedulers

**Principle (from Kotlin/Loom)**: Every task has a well-defined owner. When the owner is cancelled, all its tasks are cancelled.

In syncopate, sub-schedulers are the scoping mechanism:

```rust
// Create a sub-scheduler for the physics component
let physics = handle.spawn_sub_scheduler(SubSchedulerConfig {
    period: Duration::from_millis(200),
    allow_negotiation: false,
    failure_policy: FailurePolicy::Continue,
})?;

// Add tasks owned by this sub-scheduler
physics.add_task(TaskConfig::periodic(Duration::from_millis(200)).name("step"))?;
physics.add_task(TaskConfig::periodic(Duration::from_secs(1)).name("debug_draw"))?;

// When the physics component is done:
handle.remove_sub_scheduler(physics.id())?;
// ^ Both "step" and "debug_draw" are cancelled. No orphans.
```

**Cascading shutdown**: When the root scheduler shuts down, all sub-schedulers and their tasks are cancelled in reverse creation order (children before parents).

### Fault Isolation via Failure Policies

**Principle (from Erlang/OTP)**: A fault in one component should not corrupt other components. A supervisor decides what to do when a child fails.

In syncopate, `FailurePolicy` controls what happens when a callback panics:

```rust
// "Let it crash" — isolate the failure, continue with other tasks
FailurePolicy::Continue

// Stop the sub-scheduler on first panic
FailurePolicy::Stop

// Restart with backoff (Erlang-style intensity limiting)
FailurePolicy::Restart {
    max_restarts: 3,
    within: Duration::from_secs(60),
}
```

**Panic catching**: Callbacks in sub-schedulers are wrapped in `std::panic::catch_unwind()`. The panic is logged, the task is marked as failed, and the failure policy is applied. The parent scheduler's state is never corrupted.

**Root scheduler callbacks**: Callbacks added directly to the root scheduler (not via a sub-scheduler) are NOT wrapped in catch_unwind by default. A panicking root callback will propagate the panic to the application's `poll()` call. This is intentional — the root scheduler has no supervisor to report to.

### Sub-Scheduler Hierarchy Depth

Sub-schedulers can be nested arbitrarily deep. Each level provides its own fault isolation boundary:

```
Root Scheduler (min_period=10ms)
  +-- Physics Sub (period=20ms, FailurePolicy::Restart)
  |     +-- Collision Sub (period=40ms, FailurePolicy::Continue)
  |     +-- Kinematics Sub (period=20ms, FailurePolicy::Continue)
  +-- AI Sub (period=1s, FailurePolicy::Stop)
  +-- Telemetry Sub (period=60s, FailurePolicy::Continue)
```

If a Collision task panics:
1. Collision sub-scheduler applies `Continue` — the panicking task is marked failed, other collision tasks continue
2. Physics sub-scheduler is unaffected
3. AI and Telemetry are completely isolated

If the Physics sub-scheduler itself exceeds its restart limit:
1. Physics and all its children (Collision, Kinematics) are stopped
2. AI and Telemetry are unaffected
3. Root scheduler reports a `SubSchedulerFailed` event in the next `PollResult`

---

## Coalescing Algorithm (Efficiency Tier)

The v1 design used an O(n*m) grid search: generate candidate wakeup times on a frequency grid, then score each candidate against all tasks. This is replaced with an **O(n log n) weighted interval sweep**.

### Algorithm: Weighted Interval Sweep

**Input:** Set of tasks with timing windows `[earliest, latest]` and priorities.

**Output:** Optimal wakeup time `t*` that maximizes the weighted count of tasks whose windows include `t*`.

```
1. For each task i with window [earliest_i, latest_i] and priority p_i:
     weight_i = 1.0 / (p_i + 1)   // higher priority -> higher weight
     Emit events:
       (earliest_i, +weight_i)    // window opens
       (latest_i,   -weight_i)    // window closes

2. Sort all events by time.                          // O(n log n)

3. Sweep through events, maintaining cumulative weight:
     current_weight = 0
     best_weight = 0
     best_time = now

     for (time, delta) in events:
       current_weight += delta
       if current_weight > best_weight:
         best_weight = current_weight
         best_time = time

4. Return best_time as the optimal wakeup point.
```

### Why This Works

The maximum of the cumulative weight function occurs at the time where the most high-priority windows overlap. This is exactly the point where waking up would cover the most tasks with the least number of wakeups.

### Comparison

| Property | V1 (Grid Search) | V2 (Interval Sweep) |
|----------|-----------------|---------------------|
| Time complexity | O(n * m) where m = grid points | O(n log n) |
| Space complexity | O(m) candidates | O(n) events |
| Resolution | Limited by grid spacing | Continuous (exact window boundaries) |
| Frequency dependency | Requires frequency bounds for grid | Independent of frequency bounds |

### Refinement: Prefer Ideal Times

When multiple times yield the same weight, prefer the one closest to the weighted center of ideal times for the covered tasks. This minimizes average jitter.

---

## Performance Considerations

### Time Complexity

| Operation | Precision Tier | Efficiency Tier | Notes |
|-----------|---------------|-----------------|-------|
| `poll()` (no due tasks) | O(1) | O(1) | Peek heap / check cached wakeup |
| `poll()` (k due tasks) | O(k log n) | O(n log n) | Drain k from heap / full sweep |
| Add task | O(log n) | O(log n) | Insert into priority heap |
| Remove task | O(1) amortized | O(1) amortized | Mark removed in arena, lazy cleanup |
| Modify task | O(log n) | O(log n) | Remove + re-insert |
| `max_idle_duration()` | O(1) | O(1) | Atomic read |
| `mark_completed()` | O(k log n) | O(k log n) | Reschedule k periodic tasks |
| Period negotiation | O(n) | O(n) | Recompute GCD of all periods |

### Memory Usage

**Per Task:**
- `TaskConfig`: ~96 bytes (with callback Arcs and budget)
- Internal metadata (next deadline, state, generation): ~48 bytes
- Total: ~144 bytes per task

**Per Scheduler:**
- Priority heaps: O(n) where n = number of tasks
- Sub-scheduler tree: O(s) where s = number of sub-schedulers
- Command channel buffer: configurable, default 256 commands
- Atomic state (idle duration, next wakeup): 16 bytes

**Total:** For 1,000 tasks, ~144 KB. For 10,000 tasks, ~1.4 MB.

### Benchmark Targets (Raspberry Pi 5)

| Metric | Precision Tier | Efficiency Tier |
|--------|---------------|-----------------|
| `poll()` latency, 100 tasks | < 5 us | < 50 us |
| `poll()` latency, 1,000 tasks | < 20 us | < 500 us |
| `poll()` latency, 10,000 tasks | < 100 us | < 5 ms |
| `add_task` latency | < 1 us | < 1 us |
| `remove_task` latency | < 1 us | < 1 us |
| Memory per task | < 144 bytes | < 144 bytes |
| Coalescing efficiency (1,000 tasks) | N/A | > 80% |

---

## Implementation Roadmap

### Phase 1: Core Foundation (Weeks 1-3)

**Status: Partially implemented.**

**Deliverables:**
- `TaskConfig` with convenience constructors (`periodic()`, `periodic_hz()`, `after()`, etc.)
- `TaskType::Periodic` with asymmetric timing windows and `PhaseMode::Relative` (default)
- `TaskId` with generation counters
- `PollResult` with `idle_duration`, `next_wakeup`, `executed`, `due`, `missed`
- `SchedulerCore` with `BinaryHeap`-based EDF (single priority level initially)
- `SchedulerHandle` + `SchedulerLoop` with `crossbeam` channel
- `SchedulerBuilder` with `with_context()` and `build_local()`
- Callback-based execution (`on_execute`, `on_miss`)
- Rename `with_executor()` to `on_execute()` for clarity (avoid confusion with async executor terminology)

**Testing focus:**
- Scheduling accuracy (task appears at the right time)
- Window boundaries (tasks within and outside windows)
- Idle duration correctness
- Handle/loop command processing
- Callback invocation with correct `TaskExecution` / `TaskMiss` data

### Phase 2: Task Lifecycle + One-Shot + Anchored Scheduling (Weeks 4-6)

**Deliverables:**
- `remove_task()` — task cancellation and slot reuse with generation counter
- `pause_task()` / `resume_task()` — suspend scheduling without removal
- `modify_task()` with `TaskPatch` — change period, priority, windows at runtime
- `TaskType::OneShot` with `Deadline::Monotonic` — relative one-shot tasks ("fire in 5 minutes")
- `Deadline::WallClock(SystemTime)` — anchored one-shot tasks ("fire at 12:45pm")
- `PhaseMode::Anchored` — anchored periodic tasks with wall-clock phase locking
- `TimeReference` — monotonic ↔ wall-clock time mapping, captured at construction
- Convenience constructors: `TaskConfig::one_shot()`, `TaskConfig::after()`, `TaskConfig::at_wall_clock()`, `TaskConfig::periodic_anchored()`, `TaskConfig::periodic_hz()`, `TaskConfig::periodic_hz_anchored()`

**Testing focus:**
- Remove + re-add with same slot (generation counter prevents ABA)
- Pause/resume preserves deadline state
- Modify while paused
- One-shot lifecycle (fires once, then auto-removed)
- Removed task produces `TaskNotFound` on subsequent operations
- Anchored task phase computation (first deadline after `now`)
- `TimeReference` wall ↔ mono conversion accuracy
- Anchored periodic task fires at correct wall-clock boundaries

### Phase 3: Priority Lanes + Aging (Weeks 7-8)

**Deliverables:**
- Multi-level priority heaps (one `BinaryHeap` per priority level)
- `PriorityClass` enum with semantic names
- Priority aging with configurable `AgingPolicy`
- Due tasks sorted by priority then deadline in `PollResult`

**Testing focus:**
- Priority ordering in `PollResult.due` and `PollResult.executed`
- Starvation prevention with aging enabled
- Mixed-priority scheduling fairness

### Phase 4: Efficiency Tier + Alignment Algorithm (Weeks 9-11)

**Deliverables:**
- O(n log n) weighted interval sweep (coalescing algorithm)
- `SchedulerTier::Efficiency` mode
- Configurable tier selection in builder
- Coalescing metrics in `SchedulerStats`
- Idle duration reporting optimized for coalesced wakeups
- **Anchor grid** computation (GCD of anchored task periods)
- **Gradual alignment algorithm** — shift relative tasks toward anchor grid points
- **First-tick validation** — reject tasks that can't coalesce on first tick (`NotCoalescable` error)
- Grid-aware idle duration computation (prefer waking at grid points during convergence)

**Testing focus:**
- Coalescing effectiveness (tasks per wakeup)
- Jitter bounds respected
- Priority weighting in coalescing decisions
- Comparison with precision tier on same workload
- Alignment convergence: relative task converges to grid within expected cycles
- Alignment stability: aligned task stays on grid
- First-tick rejection: task with too-narrow window rejected with correct `distance`
- First-tick acceptance: task with sufficient window accepted
- Empty scheduler always accepts first task
- Grid recomputation when anchored tasks added/removed
- Mixed relative+anchored workload coalescing efficiency

### Phase 5: Hierarchical Sub-Schedulers + Structured Concurrency (Weeks 12-14)

**Deliverables:**
- `spawn_sub_scheduler()` with relaxed period constraint (child period >= parent period)
- `SubSchedulerHandle` for adding tasks to children
- `remove_sub_scheduler()` with cascading cancellation (structured concurrency)
- Parent-child tick coordination (parent `poll()` includes children's due tasks)
- `FailurePolicy` with `Continue`, `Stop`, `Restart` strategies
- `catch_unwind()` for callbacks in sub-schedulers
- Sub-scheduler pause/resume

**Testing focus:**
- Period constraint enforcement
- Cascading cancellation (remove sub-scheduler cancels all children)
- Fault isolation (panic in child doesn't corrupt parent)
- Restart policy with intensity limiting
- Multi-level hierarchy (grandchild schedulers)
- Shutdown cascade (root shutdown cancels all sub-schedulers)

### Phase 6: Period Negotiation (Week 15)

**Deliverables:**
- Request/response protocol via channel
- Global period recomputation (GCD on `u64` nanoseconds)
- `NegotiationError` reporting
- Policy enforcement (per-child allow/deny)

**Testing focus:**
- Approval/denial logic
- Global period changes propagate correctly
- No deadline misses caused by period change

### Phase 7: Clock Trait + Clock Jump Detection (Weeks 16-17)

**Deliverables:**
- `Clock` trait with default `StdClock` implementation
- `MockClock` for deterministic testing
- `Sleeper` trait (informational — the application uses it, the scheduler doesn't)
- Clock-jump detection: compare expected vs actual wall-clock time each poll, re-anchor affected tasks when jump exceeds threshold
- `build_with_clock()` builder method
- `clock_jump_threshold()` builder configuration

**Testing focus:**
- All existing tests converted to `MockClock` for determinism
- Clock-jump handling: forward jump re-anchors correctly
- Clock-jump handling: backward jump re-anchors correctly
- `MockClock::advance()` for time-controlled tests
- Relative tasks unaffected by simulated clock jumps
- Anchored tasks correctly re-anchored after simulated clock jumps

### Phase 8: Optimization + Polish (Weeks 18-20)

**Deliverables:**
- Benchmarks on Raspberry Pi 5 hardware
- Arena allocator for `TaskStorage` (avoid per-task heap allocation)
- Lock-free `AtomicU64` for `idle_duration` and `next_wakeup` reads
- `callback_budget` with slow-callback detection and logging
- Feature flags: `tokio-sleep` (provides a `Sleeper` impl using `tokio::time`), `std-sleep` (provides `thread::sleep` impl)
- Documentation, examples, integration tests
- `thiserror` for error types

**Testing focus:**
- Performance regression tests
- Memory allocation profiling
- Concurrency stress tests (many handles, one loop)
- Slow-callback detection tests
- Alignment convergence benchmarks (time to converge with various window sizes)

---

## Future Work

Items deferred beyond v1:

1. **Task Dependencies**
   - Express "task B must run after task A"
   - DAG-based scheduling within a single poll cycle

2. **Dynamic Priority Adjustment**
   - User-defined priority functions
   - Adaptive priority based on miss rates

3. **Task Groups**
   - Atomic execution: all tasks in a group appear in `due` together or not at all

4. **Task Constraints** (inspired by Kepler SDK)
   - `requires_foreground` — only schedule when app is in foreground
   - `requires_charging` — only schedule when device is charging
   - `power_mode: PowerMode` — hint for coalescing aggressiveness
   - These are common in mobile/IoT SDKs and would enhance the efficiency tier

5. **Retry Timer Abstraction** (inspired by Kepler's `IRetryTimer`)
   - Exponential backoff with configurable base, max, multiplier, and jitter
   - Built on one-shot tasks that reschedule themselves

6. **Distributed Scheduling**
   - Multi-process coordination via shared memory
   - Distributed coalescing

7. **Energy Profiling**
   - Measure actual power consumption per coalescing strategy
   - Hardware-specific optimization

8. **no_std Support**
   - Remove `std::time` dependency, use generic clock
   - Target embedded systems (Cortex-M)

9. **Tracing Integration**
   - `tracing` crate for observability
   - Export scheduling decisions as spans

10. **Metrics Export**
    - Prometheus metrics
    - Grafana dashboards

11. **SIMD Coalescing**
    - Vectorize the interval sweep for very large task sets (10,000+)

12. **Formal Verification**
    - TLA+ model of the coalescing algorithm
    - Prove no-starvation property with aging enabled

13. **Async Callbacks**
    - Support `async fn` callbacks (requires integration with an async runtime)
    - Would need a `spawn`-like trait, which conflicts with the runtime-agnostic goal
    - Possible via `fn(&Ctx) -> Pin<Box<dyn Future>>` with explicit runtime integration

---

## References

### Academic Papers

1. **Regehr, J. (2001)** — "Using Hierarchical Scheduling to Support Soft Real-Time Applications in General-Purpose Operating Systems"
   - Foundation for hierarchical scheduler design

2. **Liu, C. L., & Layland, J. W. (1973)** — "Scheduling Algorithms for Multiprogramming in a Hard-Real-Time Environment"
   - Introduced Rate Monotonic Scheduling and EDF

3. **Varghese, G., & Lauck, A. (1987)** — "Hashed and Hierarchical Timing Wheels"
   - Timer wheel data structures (used by Tokio internally)

4. **Khalilzad, M. M. (2013)** — "Adaptive Hierarchical Scheduling Framework"
   - Resource negotiation in hierarchical schedulers

### Hardware Documentation

5. **ARM Architecture Reference Manual (ARMv8-A)** — Generic Timer documentation
   - `CNTPCT_EL0` counter, `CNTP_TVAL_EL0` timer value
   - Counter frequency (`CNTFRQ_EL0`): typically 54 MHz on BCM2712

6. **Raspberry Pi 5 (BCM2712) Datasheet** — Timer and interrupt controller
   - GIC-400 interrupt latency characteristics

### Industry Resources

7. **Linux Kernel Scheduling Domains** — https://lwn.net/Articles/80911/
   - Multi-level scheduling in Linux

8. **PREEMPT_RT Wiki** — https://wiki.linuxfoundation.org/realtime/
   - Real-time Linux scheduling characteristics

9. **W3C Prioritized Task Scheduling API** — https://wicg.github.io/scheduling-apis/
   - Browser priority scheduling

### Rust Ecosystem

10. **Tokio Runtime Documentation** — https://docs.rs/tokio/latest/tokio/runtime/
    - Work-stealing executor, JoinHandle, CancellationToken, JoinSet

11. **crossbeam** — https://docs.rs/crossbeam/
    - Lock-free channel implementations

12. **smol** — https://docs.rs/smol/
    - Minimal async runtime, composable design philosophy

13. **embedded-flight-scheduler** — https://docs.rs/embedded-flight-scheduler/
    - Real-time embedded scheduler example

### Other Language Runtimes

14. **Go Runtime Scheduler** — https://go.dev/src/runtime/proc.go
    - M:N scheduling, context.Context, time.Ticker, preemptive scheduling at safepoints (Go 1.14)

15. **Kotlin Coroutines** — https://kotlinlang.org/docs/coroutines-guide.html
    - Structured concurrency, CoroutineScope, SupervisorJob, withTimeout()

16. **Java Project Loom** — https://openjdk.org/jeps/444
    - Virtual threads, StructuredTaskScope, ShutdownOnFailure/ShutdownOnSuccess

17. **Python asyncio** — https://docs.python.org/3/library/asyncio.html
    - Event loop, TaskGroup (3.11+), call_later(), asyncio.timeout()

18. **Erlang/OTP Supervision** — https://www.erlang.org/doc/design_principles/sup_princ
    - Supervision trees, restart strategies, fault isolation, reduction-based preemption

---

## Appendix A: Coalescing Algorithm (Detailed Implementation)

```rust
use std::time::{Duration, Instant};

/// A task window for coalescing consideration.
struct CoalesceWindow {
    earliest: Instant,
    latest: Instant,
    priority: u8,
}

/// Event in the sweep line algorithm.
#[derive(PartialEq)]
struct SweepEvent {
    time: Instant,
    weight_delta: f64, // positive for window open, negative for window close
}

impl Eq for SweepEvent {}

impl PartialOrd for SweepEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SweepEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

/// Find the optimal wakeup time using weighted interval sweep.
/// Returns the time that maximizes the weighted count of covered tasks.
fn find_optimal_wakeup(windows: &[CoalesceWindow], now: Instant) -> Instant {
    if windows.is_empty() {
        return now;
    }

    // 1. Generate sweep events
    let mut events: Vec<SweepEvent> = Vec::with_capacity(windows.len() * 2);

    for w in windows {
        let weight = 1.0 / (w.priority as f64 + 1.0);
        events.push(SweepEvent {
            time: w.earliest,
            weight_delta: weight,
        });
        events.push(SweepEvent {
            time: w.latest,
            weight_delta: -weight,
        });
    }

    // 2. Sort by time
    events.sort();

    // 3. Sweep to find maximum weight point
    let mut current_weight: f64 = 0.0;
    let mut best_weight: f64 = 0.0;
    let mut best_time = now;

    for event in &events {
        current_weight += event.weight_delta;
        if current_weight > best_weight {
            best_weight = current_weight;
            best_time = event.time;
        }
    }

    best_time
}
```

---

## Appendix B: GCD/LCM Computation on Integer Nanoseconds

```rust
use std::time::Duration;

/// Compute greatest common divisor of two u64 values.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Compute least common multiple of two u64 values.
fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    (a / gcd(a, b)) * b // avoid overflow: divide first
}

/// Compute the optimal global tick period from a set of task periods.
/// Returns the GCD of all periods (in nanoseconds), which is the finest
/// granularity that evenly divides all task periods.
fn compute_global_period(periods: &[Duration]) -> Duration {
    if periods.is_empty() {
        return Duration::from_secs(1);
    }

    let periods_ns: Vec<u64> = periods
        .iter()
        .map(|d| d.as_nanos() as u64)
        .collect();

    let gcd_ns = periods_ns
        .iter()
        .copied()
        .reduce(gcd)
        .unwrap();

    Duration::from_nanos(gcd_ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_period_computation() {
        let periods = vec![
            Duration::from_secs(1),       // 1,000,000,000 ns
            Duration::from_millis(500),    //   500,000,000 ns
            Duration::from_millis(250),    //   250,000,000 ns
        ];

        let global = compute_global_period(&periods);

        // GCD(1000ms, 500ms, 250ms) = 250ms
        assert_eq!(global, Duration::from_millis(250));
    }

    #[test]
    fn test_non_divisor_periods() {
        let periods = vec![
            Duration::from_millis(100),  // 100,000,000 ns
            Duration::from_millis(150),  // 150,000,000 ns
        ];

        let global = compute_global_period(&periods);

        // GCD(100ms, 150ms) = 50ms
        assert_eq!(global, Duration::from_millis(50));
    }
}
```

---

## Appendix C: Convenience Constructor Implementation

```rust
impl<Ctx> TaskConfig<Ctx> {
    /// Create a periodic task with default settings (relative phase).
    pub fn periodic(period: Duration) -> Self {
        TaskConfig {
            task_type: TaskType::Periodic {
                period,
                window_before: Duration::ZERO,
                window_after: Duration::ZERO,
                phase: PhaseMode::Relative,
            },
            priority: PriorityClass::Normal.as_u8(),
            name: None,
            on_execute: None,
            on_miss: None,
            callback_budget: None,
        }
    }

    /// Create a periodic task with symmetric window (relative phase).
    pub fn periodic_with_window(period: Duration, tolerance: Duration) -> Self {
        TaskConfig {
            task_type: TaskType::Periodic {
                period,
                window_before: tolerance,
                window_after: tolerance,
                phase: PhaseMode::Relative,
            },
            priority: PriorityClass::Normal.as_u8(),
            name: None,
            on_execute: None,
            on_miss: None,
            callback_budget: None,
        }
    }

    /// Create a periodic task from a frequency in Hz (relative phase).
    /// Converts to Duration internally: period = 1_000_000_000 / hz nanoseconds.
    /// Panics if hz is 0.
    pub fn periodic_hz(hz: u64) -> Self {
        assert!(hz > 0, "Hz must be > 0");
        let period_ns = 1_000_000_000u64 / hz;
        Self::periodic(Duration::from_nanos(period_ns))
    }

    /// Create a periodic task anchored to wall-clock boundaries.
    /// `anchor` defines phase=0. First deadline = anchor + k*period
    /// where k is the smallest integer making the deadline in the future.
    pub fn periodic_anchored(period: Duration, anchor: SystemTime) -> Self {
        TaskConfig {
            task_type: TaskType::Periodic {
                period,
                window_before: Duration::ZERO,
                window_after: Duration::ZERO,
                phase: PhaseMode::Anchored { anchor },
            },
            priority: PriorityClass::Normal.as_u8(),
            name: None,
            on_execute: None,
            on_miss: None,
            callback_budget: None,
        }
    }

    /// Create an anchored periodic task from Hz + wall-clock anchor.
    /// Panics if hz is 0.
    pub fn periodic_hz_anchored(hz: u64, anchor: SystemTime) -> Self {
        assert!(hz > 0, "Hz must be > 0");
        let period_ns = 1_000_000_000u64 / hz;
        Self::periodic_anchored(Duration::from_nanos(period_ns), anchor)
    }

    /// Create a one-shot task at a monotonic instant (relative).
    pub fn one_shot(deadline: Instant) -> Self {
        TaskConfig {
            task_type: TaskType::OneShot {
                deadline: Deadline::Monotonic(deadline),
                window_before: Duration::ZERO,
                window_after: Duration::ZERO,
            },
            priority: PriorityClass::Normal.as_u8(),
            name: None,
            on_execute: None,
            on_miss: None,
            callback_budget: None,
        }
    }

    /// Create a one-shot task firing after a delay from now (relative).
    pub fn after(delay: Duration) -> Self {
        Self::one_shot(Instant::now() + delay)
    }

    /// Create a one-shot task at a wall-clock time (anchored).
    pub fn at_wall_clock(time: SystemTime) -> Self {
        TaskConfig {
            task_type: TaskType::OneShot {
                deadline: Deadline::WallClock(time),
                window_before: Duration::ZERO,
                window_after: Duration::ZERO,
            },
            priority: PriorityClass::Normal.as_u8(),
            name: None,
            on_execute: None,
            on_miss: None,
            callback_budget: None,
        }
    }

    /// Set asymmetric timing window.
    pub fn window(mut self, before: Duration, after: Duration) -> Self {
        match &mut self.task_type {
            TaskType::Periodic { window_before, window_after, .. } => {
                *window_before = before;
                *window_after = after;
            }
            TaskType::OneShot { window_before, window_after, .. } => {
                *window_before = before;
                *window_after = after;
            }
        }
        self
    }

    /// Set priority level (0 = highest).
    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Set priority using a semantic class.
    pub fn priority_class(mut self, class: PriorityClass) -> Self {
        self.priority = class.as_u8();
        self
    }

    /// Set human-readable name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}
```

---

**End of Document**
