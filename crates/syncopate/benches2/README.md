# Syncopate Benchmarks

This directory contains benchmarks and analysis tools for the Syncopate scheduler.

## Benchmarks

### `tick_overhead` - Execution Speed Benchmarks

Measures **execution speed** (throughput/latency) of the scheduler using divan.

Run with:
```bash
cargo bench --bench tick_overhead
```

**What it measures:**
- Ticks per second across different workloads
- Overhead of task scheduling operations
- Scaling behavior with increasing task counts

**Scenarios:**
- 10, 100 relative periodic tasks
- 50 absolute periodic tasks (boundary-aligned and offset)
- Mixed workloads combining different task types
- Dynamic task addition during execution
- Wakeup efficiency with different window configurations

---

### `scheduling_analysis` - Scheduling Efficiency Analysis

Measures **scheduling efficiency** - how well the scheduler optimizes wakeup frequency and timing.

Run with:
```bash
cargo bench --bench scheduling_analysis
```

**What it measures:**
- **Wakeup frequency**: Total number of scheduler wakeups over a simulation period
- **Timing divergence**: How much tasks deviate from their ideal fire times
  - Average divergence across all task firings
  - Maximum divergence (worst-case timing error)

**Key Insights:**
- Compares naive scheduling (Window::ZERO, no coalescing) vs optimized (with windows)
- Shows the trade-off between wakeup reduction and timing precision
- Demonstrates how window size affects both metrics
- Highlights how task period relationships (harmonic vs inharmonic) impact efficiency

**Scenarios Analyzed:**
1. **Three Tasks (1.0s, 1.1s, 1.2s)** - Simple closely-spaced tasks
2. **Ten Tasks (0.85s-1.3s range)** - More complex inharmonic periods
3. **Harmonic Periods (500ms, 1000ms, 2000ms)** - Naturally coalescing tasks
4. **Window Size Sensitivity** - How window size affects metrics
5. **Many Tasks (20 tasks)** - Larger workload scaling

**Example Output:**
```
╔════════════════╦════════════════════╦══════════════════════════╦═════════════╗
║ Metric         ║ Naive (no windows) ║ Optimized (with windows) ║ Improvement ║
╠════════════════╪════════════════════╪══════════════════════════╪═════════════╣
║ Total Wakeups  ║ 81                 ║ 20                       ║ 75.3% fewer ║
╟────────────────╫────────────────────╫──────────────────────────╫─────────────╢
║ Avg Divergence ║ 0.000ms            ║ 171.978ms                ║ +171.978ms  ║
╟────────────────╫────────────────────╫──────────────────────────╫─────────────╢
║ Max Divergence ║ 0.000ms            ║ 750.000ms                ║ +750.000ms  ║
╚════════════════╩════════════════════╩══════════════════════════╩═════════════╝
```

---

## Differences Between Benchmarks

| Aspect | tick_overhead | scheduling_analysis |
|--------|--------------|---------------------|
| **Focus** | Execution speed | Scheduling efficiency |
| **Framework** | divan (traditional benchmarking) | Custom analysis (single simulation run) |
| **Metrics** | Iterations/sec, ns/operation | Wakeup count, timing divergence |
| **Output** | Statistical timing data | Human-readable comparison tables |
| **Purpose** | Optimize code performance | Optimize scheduling algorithms |
| **Run Time** | Multiple iterations for statistics | Single comprehensive simulation |

## When to Use Each

- **Use `tick_overhead`** when:
  - Optimizing scheduler code performance
  - Checking for performance regressions
  - Comparing different implementation approaches
  - Measuring impact of code changes on execution speed

- **Use `scheduling_analysis`** when:
  - Evaluating task coalescing effectiveness
  - Choosing optimal window sizes for applications
  - Understanding power efficiency trade-offs
  - Analyzing timing precision requirements
  - Demonstrating scheduler behavior to users

## Running All Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run only speed benchmarks
cargo bench --bench tick_overhead

# Run only efficiency analysis
cargo bench --bench scheduling_analysis
```
