use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::{TaskBuilder, Window};

/// Statistics for a single task over the simulation
#[derive(Debug, Clone)]
struct TaskStats {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    period: Duration,
    #[allow(dead_code)]
    window: Window,
    #[allow(dead_code)]
    fire_count: usize,
    divergences: Vec<i64>, // in microseconds, can be negative (early) or positive (late)
}

impl TaskStats {
    #[allow(dead_code)]
    fn avg_divergence(&self) -> f64 {
        if self.divergences.is_empty() {
            return 0.0;
        }
        let sum: i64 = self.divergences.iter().sum();
        (sum as f64) / (self.divergences.len() as f64) / 1000.0 // convert to ms
    }

    fn max_divergence(&self) -> f64 {
        self.divergences.iter().max().copied().unwrap_or(0) as f64 / 1000.0
    }

    #[allow(dead_code)]
    fn min_divergence(&self) -> f64 {
        self.divergences.iter().min().copied().unwrap_or(0) as f64 / 1000.0
    }
}

/// Wakeup event for debugging
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct WakeupEvent {
    time_ms: u64,
    tasks_fired: Vec<String>,
    coalesced_count: usize,
}

/// Simulation results for a scenario
#[derive(Debug)]
struct SimulationResult {
    #[allow(dead_code)]
    scenario_name: String,
    total_wakeups: usize,
    #[allow(dead_code)]
    simulation_duration: Duration,
    task_stats: Vec<TaskStats>,
}

impl SimulationResult {
    /// Average divergence across all task firings
    fn avg_divergence_global(&self) -> f64 {
        let all_divergences: Vec<i64> = self
            .task_stats
            .iter()
            .flat_map(|ts| &ts.divergences)
            .copied()
            .collect();

        if all_divergences.is_empty() {
            return 0.0;
        }

        let sum: i64 = all_divergences.iter().sum();
        (sum as f64) / (all_divergences.len() as f64) / 1000.0
    }

    fn max_divergence_global(&self) -> f64 {
        self.task_stats
            .iter()
            .map(|ts| ts.max_divergence())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }
}

/// Tracks expected fire times for a task
struct TaskTracker {
    name: String,
    period: Duration,
    window: Window,
    next_ideal_fire: Duration,
    fire_count: usize,
    divergences: Vec<i64>,
}

/// Run simulation and track timing statistics
fn simulate_with_tracking(
    tasks: Vec<(String, Duration, Window)>,
    simulation_duration: Duration,
    verbose: bool,
) -> SimulationResult {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

    // Track expected fire times for each task
    let mut task_trackers: Vec<TaskTracker> = tasks
        .iter()
        .enumerate()
        .map(|(idx, (name, period, window))| {
            let task = TaskBuilder::every(*period, *window)
                .name(format!("task_{}", idx))
                .build()
                .unwrap();

            scheduler.add_task(task).unwrap();

            TaskTracker {
                name: name.clone(),
                period: *period,
                window: *window,
                next_ideal_fire: *period,
                fire_count: 0,
                divergences: Vec::new(),
            }
        })
        .collect();

    let mut current_time = epoch;
    let end_time = epoch + simulation_duration;
    let mut wakeup_count = 0;

    // Simulation loop
    while current_time < end_time {
        let next_tick_duration = match scheduler.calculate_next_tick() {
            Some(d) if d > Duration::ZERO => d,
            _ => Duration::from_millis(1), // Fall back to 1ms minimum (matches tick_overhead.rs)
        };

        current_time += next_tick_duration;
        if current_time >= end_time {
            break;
        }

        scheduler.advance_time(current_time);
        let fired = scheduler.tick(next_tick_duration);

        if !fired.is_empty() {
            wakeup_count += 1;

            // Track which tasks fired and calculate divergence
            let actual_fire_time = current_time.duration_since(epoch).unwrap();

            if verbose {
                let time_ms = actual_fire_time.as_millis();
                println!(
                    "\n>>> Wakeup #{} at {}ms: {} tasks fired",
                    wakeup_count,
                    time_ms,
                    fired.len()
                );
            }

            // Calculate divergences first for verbose output
            let mut task_divergences = Vec::new();

            for task in &fired {
                // Find the corresponding tracker by matching task index from name
                if let Some(task_name) = &task.name {
                    if let Some(idx) = task_name
                        .strip_prefix("task_")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(tracker) = task_trackers.get_mut(idx) {
                            // Calculate divergence: actual - ideal (in microseconds)
                            let ideal_micros = tracker.next_ideal_fire.as_micros() as i64;
                            let actual_micros = actual_fire_time.as_micros() as i64;
                            let divergence = actual_micros - ideal_micros;

                            if verbose {
                                let ideal_ms = tracker.next_ideal_fire.as_millis();
                                let actual_ms = actual_fire_time.as_millis();
                                let divergence_ms = divergence as f64 / 1000.0;
                                task_divergences.push((
                                    tracker.name.clone(),
                                    idx,
                                    ideal_ms,
                                    actual_ms,
                                    divergence_ms,
                                ));
                            }

                            tracker.divergences.push(divergence);
                            tracker.fire_count += 1;

                            // Update next ideal fire time
                            tracker.next_ideal_fire += tracker.period;
                        }
                    }
                }
            }

            if verbose {
                // Print detailed divergence info
                for (name, idx, ideal_ms, actual_ms, divergence_ms) in task_divergences {
                    println!(
                        "    Task {} [{}]: ideal={}ms, actual={}ms, divergence={:+.3}ms",
                        idx, name, ideal_ms, actual_ms, divergence_ms
                    );
                }
            }
        }
    }

    SimulationResult {
        scenario_name: String::from("Simulation"),
        total_wakeups: wakeup_count,
        simulation_duration,
        task_stats: task_trackers
            .into_iter()
            .map(|t| TaskStats {
                name: t.name,
                period: t.period,
                window: t.window,
                fire_count: t.fire_count,
                divergences: t.divergences,
            })
            .collect(),
    }
}

/// Print comparison table between naive and optimized approaches
fn print_comparison(naive: &SimulationResult, optimized: &SimulationResult) {
    use comfy_table::{Table, presets::UTF8_FULL};

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        "Metric",
        "Naive (no windows)",
        "Optimized (with windows)",
        "Improvement",
    ]);

    // Total wakeups
    let wakeup_improvement = if naive.total_wakeups > 0 {
        let diff = optimized.total_wakeups as i64 - naive.total_wakeups as i64;
        let percent = (diff as f64 / naive.total_wakeups as f64) * 100.0;
        if diff < 0 {
            format!("{:.1}% fewer", -percent)
        } else if diff > 0 {
            format!("{:.1}% more", percent)
        } else {
            "no change".to_string()
        }
    } else {
        "n/a".to_string()
    };

    table.add_row(vec![
        "Total Wakeups",
        &naive.total_wakeups.to_string(),
        &optimized.total_wakeups.to_string(),
        &wakeup_improvement,
    ]);

    // Average divergence
    table.add_row(vec![
        "Avg Divergence",
        &format!("{:.3}ms", naive.avg_divergence_global()),
        &format!("{:.3}ms", optimized.avg_divergence_global()),
        &format!(
            "{:+.3}ms",
            optimized.avg_divergence_global() - naive.avg_divergence_global()
        ),
    ]);

    // Max divergence
    table.add_row(vec![
        "Max Divergence",
        &format!("{:.3}ms", naive.max_divergence_global()),
        &format!("{:.3}ms", optimized.max_divergence_global()),
        &format!(
            "{:+.3}ms",
            optimized.max_divergence_global() - naive.max_divergence_global()
        ),
    ]);

    println!("{}", table);
}

/// Print detailed statistics for each task
#[allow(dead_code)]
fn print_detailed_stats(result: &SimulationResult) {
    println!("\nDetailed Task Statistics:");
    println!("{}", "-".repeat(80));

    for task in &result.task_stats {
        println!(
            "\nTask: {} (period: {:?}, window: {:?})",
            task.name, task.period, task.window
        );
        println!("  Fires: {}", task.fire_count);
        println!("  Avg divergence: {:.3}ms", task.avg_divergence());
        println!("  Max divergence: {:.3}ms", task.max_divergence());
        println!("  Min divergence: {:.3}ms", task.min_divergence());
    }
}

/// Analyze window overlaps mathematically to determine expected coalescing behavior
fn analyze_window_overlaps(tasks: &[(String, Duration, Window)], cycle_count: usize) {
    println!("\n{}", "=".repeat(80));
    println!("Window Overlap Analysis (Mathematical Expectations)");
    println!("{}", "=".repeat(80));

    println!("\nTask Windows:");
    for (idx, (name, period, window)) in tasks.iter().enumerate() {
        let period_ms = period.as_millis() as i64;
        let earliest = period_ms - window.early.as_millis() as i64;
        let latest = period_ms + window.late.as_millis() as i64;
        println!(
            "  Task {}: {} - period {}ms, window [{}, {}]ms",
            idx, name, period_ms, earliest, latest
        );
    }

    // Find common overlap region in first cycle
    if tasks.len() >= 2 {
        let mut max_earliest = i64::MIN;
        let mut min_latest = i64::MAX;

        for (_, period, window) in tasks {
            let period_ms = period.as_millis() as i64;
            let earliest = period_ms - window.early.as_millis() as i64;
            let latest = period_ms + window.late.as_millis() as i64;
            max_earliest = max_earliest.max(earliest);
            min_latest = min_latest.min(latest);
        }

        println!("\nFirst Cycle Analysis:");
        if max_earliest <= min_latest {
            println!(
                "  ✓ Common overlap region: [{}, {}]ms",
                max_earliest, min_latest
            );
            let midpoint = (max_earliest + min_latest) / 2;
            println!(
                "  ✓ Optimal coalescing point: {}ms (midpoint of overlap)",
                midpoint
            );
            println!("  ✓ All {} tasks can fire together", tasks.len());
            println!("\nExpected Behavior:");
            println!("  If coalescing works perfectly:");
            println!(
                "    - All tasks should fire together at ~{}ms in each period",
                midpoint
            );
            println!(
                "    - Total wakeups over {} cycles: ~{}",
                cycle_count, cycle_count
            );
        } else {
            println!(
                "  ✗ No common overlap region (earliest: {}, latest: {})",
                max_earliest, min_latest
            );
            println!("  ✗ Tasks cannot all coalesce together");

            // Check pairwise overlaps
            println!("\n  Pairwise Overlap Analysis:");
            let mut any_pairs = false;
            for i in 0..tasks.len() {
                for j in (i + 1)..tasks.len() {
                    let (name1, p1, w1) = &tasks[i];
                    let (name2, p2, w2) = &tasks[j];

                    let p1_ms = p1.as_millis() as i64;
                    let p2_ms = p2.as_millis() as i64;
                    let e1 = p1_ms - w1.early.as_millis() as i64;
                    let l1 = p1_ms + w1.late.as_millis() as i64;
                    let e2 = p2_ms - w2.early.as_millis() as i64;
                    let l2 = p2_ms + w2.late.as_millis() as i64;

                    let overlap_start = e1.max(e2);
                    let overlap_end = l1.min(l2);

                    if overlap_start <= overlap_end {
                        println!(
                            "    ✓ {} & {}: overlap [{}, {}]ms",
                            name1, name2, overlap_start, overlap_end
                        );
                        any_pairs = true;
                    } else {
                        println!("    ✗ {} & {}: NO overlap", name1, name2);
                    }
                }
            }
            if !any_pairs {
                println!("    (No pairs can coalesce)");
            }
        }
    }
    println!("{}", "=".repeat(80));
}

/// Compare naive (no windows) vs optimized (with windows)
fn run_comparison_scenario(
    scenario_name: &str,
    tasks: Vec<(String, Duration)>,
    window_size: Duration,
    simulation_duration: Duration,
    verbose: bool,
) {
    println!("\n{}", "=".repeat(80));
    println!("Scenario: {}", scenario_name);
    println!("{}", "=".repeat(80));

    // Naive: Window::ZERO (no coalescing)
    let naive_tasks: Vec<_> = tasks
        .iter()
        .map(|(name, period)| (name.clone(), *period, Window::ZERO))
        .collect();

    let naive_result = simulate_with_tracking(naive_tasks, simulation_duration, false);

    // Optimized: With windows (enables coalescing)
    let window = Window::symmetric(window_size);
    let optimized_tasks: Vec<_> = tasks
        .iter()
        .map(|(name, period)| (name.clone(), *period, window))
        .collect();

    let optimized_result = simulate_with_tracking(optimized_tasks, simulation_duration, false);

    // Print comparison
    print_comparison(&naive_result, &optimized_result);
}

fn main() {
    println!("Syncopate Scheduling Efficiency Analysis");
    println!("{}", "=".repeat(80));
    println!("This tool measures scheduling EFFICIENCY, not execution speed.");
    println!("Key metrics:");
    println!("  - Wakeup frequency: How many times the scheduler wakes up");
    println!("  - Timing divergence: How much tasks deviate from ideal fire times");
    println!("{}", "=".repeat(80));

    println!("\n{}", "=".repeat(80));
    println!("Understanding Coalescing Limitations");
    println!("{}", "=".repeat(80));
    println!("\nFor effective coalescing, window size must match period differences:");
    println!("  Period diff: 1100ms - 1000ms = 100ms");
    println!("  Window needed: ±50ms minimum (100ms total width) to just touch");
    println!("  Window ideal: ±75-100ms for good overlap\n");
    println!("Harmonic periods (500ms, 1000ms, 2000ms) naturally align:");
    println!("  GCD = 500ms, so they share common multiples");
    println!("  Even without windows, some firings coincide");
    println!("  WARNING: Windows can HURT performance for harmonic periods!");
    println!("  The coalescing algorithm overhead outweighs benefits\n");

    // Scenario 1: Harmonic periods - demonstrate natural alignment
    // Note: Windows can actually HURT performance for harmonic periods!
    run_comparison_scenario(
        "Harmonic Periods (500ms, 1000ms, 2000ms)",
        vec![
            ("task_500ms".to_string(), Duration::from_millis(500)),
            ("task_1000ms".to_string(), Duration::from_millis(1000)),
            ("task_2000ms".to_string(), Duration::from_millis(2000)),
        ],
        Duration::from_millis(50),
        Duration::from_secs(10),
        false,
    );

    // Scenario 2: Non-harmonic with small windows (minimal coalescing)
    run_comparison_scenario(
        "Non-Harmonic with Small Windows (1.0s, 1.1s, 1.2s, ±50ms)",
        vec![
            ("task_1.0s".to_string(), Duration::from_secs(1)),
            ("task_1.1s".to_string(), Duration::from_millis(1100)),
            ("task_1.2s".to_string(), Duration::from_millis(1200)),
        ],
        Duration::from_millis(50), // Too small for effective coalescing
        Duration::from_secs(10),
        false,
    );

    // Scenario 3: Non-harmonic with well-matched windows (good coalescing)
    run_comparison_scenario(
        "Non-Harmonic with Well-Matched Windows (1.0s, 1.1s, 1.2s, ±100ms)",
        vec![
            ("task_1.0s".to_string(), Duration::from_secs(1)),
            ("task_1.1s".to_string(), Duration::from_millis(1100)),
            ("task_1.2s".to_string(), Duration::from_millis(1200)),
        ],
        Duration::from_millis(100), // Better sized for period differences
        Duration::from_secs(10),
        false,
    );

    // Scenario 4: Non-harmonic with large windows (maximum coalescing)
    run_comparison_scenario(
        "Non-Harmonic with Large Windows (1.0s, 1.1s, 1.2s, ±150ms)",
        vec![
            ("task_1.0s".to_string(), Duration::from_secs(1)),
            ("task_1.1s".to_string(), Duration::from_millis(1100)),
            ("task_1.2s".to_string(), Duration::from_millis(1200)),
        ],
        Duration::from_millis(150), // Large windows for maximum coalescing
        Duration::from_secs(10),
        false,
    );

    // Scenario 5: Ten tasks with varying periods
    run_comparison_scenario(
        "Ten Tasks (0.85s-1.3s range)",
        vec![
            ("task_1000".to_string(), Duration::from_millis(1000)),
            ("task_1100".to_string(), Duration::from_millis(1100)),
            ("task_1200".to_string(), Duration::from_millis(1200)),
            ("task_900".to_string(), Duration::from_millis(900)),
            ("task_1050".to_string(), Duration::from_millis(1050)),
            ("task_1150".to_string(), Duration::from_millis(1150)),
            ("task_950".to_string(), Duration::from_millis(950)),
            ("task_1250".to_string(), Duration::from_millis(1250)),
            ("task_850".to_string(), Duration::from_millis(850)),
            ("task_1300".to_string(), Duration::from_millis(1300)),
        ],
        Duration::from_millis(50),
        Duration::from_secs(10),
        false,
    );

    // Scenario 4: Window size sensitivity analysis
    println!("\n{}", "=".repeat(80));
    println!("Window Size Sensitivity Analysis");
    println!("{}", "=".repeat(80));
    println!("\nTesting how window size affects wakeups and divergence");
    println!("Tasks: 1.0s, 1.1s, 1.2s (same as Scenario 1)\n");

    use comfy_table::{Table, presets::UTF8_FULL};
    let mut sensitivity_table = Table::new();
    sensitivity_table.load_preset(UTF8_FULL);
    sensitivity_table.set_header(vec![
        "Window Size",
        "Total Wakeups",
        "Avg Divergence",
        "Max Divergence",
    ]);

    let base_tasks = vec![
        ("A".to_string(), Duration::from_secs(1)),
        ("B".to_string(), Duration::from_millis(1100)),
        ("C".to_string(), Duration::from_millis(1200)),
    ];

    for window_ms in [0, 10, 25, 50, 75, 100] {
        let window = if window_ms == 0 {
            Window::ZERO
        } else {
            Window::symmetric(Duration::from_millis(window_ms))
        };

        let tasks: Vec<_> = base_tasks
            .iter()
            .map(|(name, period)| (name.clone(), *period, window))
            .collect();

        let result = simulate_with_tracking(tasks, Duration::from_secs(10), false);

        sensitivity_table.add_row(vec![
            if window_ms == 0 {
                "None (exact)".to_string()
            } else {
                format!("±{}ms", window_ms)
            },
            result.total_wakeups.to_string(),
            format!("{:.3}ms", result.avg_divergence_global()),
            format!("{:.3}ms", result.max_divergence_global()),
        ]);
    }

    println!("{}", sensitivity_table);

    // Scenario 6: Many tasks with small windows
    run_comparison_scenario(
        "Many Tasks (20 tasks, 800-2000ms range)",
        (0..20)
            .map(|i| (format!("task_{}", i), Duration::from_millis(800 + i * 60)))
            .collect(),
        Duration::from_millis(30),
        Duration::from_secs(10),
        false,
    );

    println!("\n{}", "=".repeat(80));
    println!("Key Insights from Analysis");
    println!("{}", "=".repeat(80));
    println!("\n1. Window Size Matters:");
    println!("   - Windows must be ≥ half the period difference for coalescing");
    println!("   - Too-small windows provide minimal benefit\n");

    println!("2. Period Relationships:");
    println!("   - Harmonic periods (1:2:4, 500ms:1000ms:2000ms) coalesce naturally");
    println!("   - Prime/coprime periods (1000ms, 1100ms, 1200ms) rarely align");
    println!("   - LCM determines maximum possible alignment");
    println!("   - For harmonic periods, Window::ZERO is often MORE efficient than windows!\n");

    println!("3. Window::ZERO Behavior:");
    println!("   - Scheduler bypasses coalescing optimization entirely");
    println!("   - Each task fires at exact scheduled time");
    println!("   - Use when timing precision is critical\n");

    println!("4. Coalescing Algorithm:");
    println!("   - Sweep algorithm finds maximum window overlaps");
    println!("   - Wakes at earliest point with most tasks ready");
    println!("   - Trades timing precision for reduced wakeup frequency\n");

    println!("{}", "=".repeat(80));
    println!("Analysis complete!");
    println!("{}", "=".repeat(80));
}
