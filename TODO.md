# TODO

## SubScheduler Design

### Core Concept

A `SubScheduler` can be created from a main `Scheduler`, passed to other threads/tasks, and operates within the parent scheduler's timing bounds.

- Wraps a full `Scheduler` internally — reuses all existing scheduling logic
- Shares the parent's clock via `Clone` (works with `Arc<SimClock>`, `Arc<RealClock>`)
- Communicates its next wakeup deadline to the parent via a shared `AtomicU64` (`Arc<SharedDeadline>`)
- Parent's `soonest_deadline()` checks sub-handles alongside its own tasks
- Tick propagation is explicit (caller-driven), not automatic

### Key Types

- **`SubScheduler<Ctx, C>`** — wraps `Scheduler` + `Arc<SharedDeadline>` + parent's `min_tick_interval` snapshot
- **`SubSchedulerHandle`** — registered in parent, reads the shared atomic deadline
- **`SharedDeadline`** — single `AtomicU64` storing monotonic nanos (`u64::MAX` = nothing pending)

### min_tick_interval Inheritance

- Sub-scheduler's effective interval = `max(parent_snapshot, own)`
- Can set a stricter (larger) interval, but not more relaxed than the parent's
- Snapshot taken at creation time (simple, predictable)

### Parent Scheduler Changes

- New field: `sub_handles: Vec<SubSchedulerHandle>`
- `soonest_deadline()` also iterates sub-handles
- `tick()` evicts dead handles (detect via `Arc::strong_count`)
- New method: `create_sub_scheduler()` (requires `C: Clone`)

### Nesting

- Supported naturally since `SubScheduler` wraps `Scheduler`
- Deadlines propagate transitively up the chain
- `min_tick_interval` stacks via `max()`

### Usage Patterns

- **Single-threaded:** caller owns both, calls `sub.should_tick()` + `sub.tick()` after parent tick
- **Multi-threaded:** sub-scheduler on another thread, parent notifies via channel when it ticks

## Bullet Time (Time Dilation)

A mode where time flows slower by a configurable factor, allowing observation of scheduler behavior in slow motion.

### Concept

- A dilation factor (e.g. `2.0x`) slows everything uniformly
- A task scheduled every 500ms actually executes every 1s at `2.0x`
- The clock itself advances slower by the same factor — from the scheduler's perspective, nothing changes; it still sees 500ms intervals
- All timing is consistent: deadlines, intervals, `soonest_deadline()` all dilate together

### Implementation

- New clock wrapper: `DilatedClock<C: Clock>` that wraps any real clock
- Reads the inner clock's elapsed time and divides by the dilation factor
- `now()` returns `start + (real_elapsed / factor)` — time appears to move slower
- Factor `1.0` = normal speed, `2.0` = half speed (2x slow-mo), `0.5` = double speed (fast-forward)
- Factor is set at construction; could optionally be an `AtomicU64` for runtime adjustment

### Properties

- Zero changes to `Scheduler` — dilation is purely a clock concern
- Composes with `SimClock` (wrap sim clock for dilated simulation) and `RealClock`
- Works transparently with SubScheduler (shared dilated clock via `Clone`)
