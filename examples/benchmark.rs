use clap::Parser;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

#[derive(Parser)]
#[command(name = "syncopate-benchmark")]
#[command(about = "Benchmark the Syncopate scheduler")]
struct Args {
    /// Duration to run the benchmark (e.g., 10s)
    #[arg(long, value_name = "DURATION")]
    duration: String,

    /// Minimum scheduler period (default: 100ms)
    #[arg(long, value_name = "DURATION", default_value = "100ms")]
    min_period: String,

    /// Maximum scheduler period (default: 2s)
    #[arg(long, value_name = "DURATION", default_value = "2s")]
    max_period: String,

    /// Task period/frequency (default: 1s)
    #[arg(long, value_name = "DURATION", default_value = "1s")]
    task_period: String,
}

fn parse_duration_with_units(s: &str) -> Result<Duration, String> {
    if s.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    // Check for multi-character units (ms, us, ns) first
    if s.ends_with("ms") {
        let num_part = &s[..s.len() - 2];
        if num_part.is_empty() {
            return Err("duration must have a numeric value before the unit".to_string());
        }
        let millis: u64 = num_part
            .parse()
            .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;
        return Ok(Duration::from_millis(millis));
    }

    if s.ends_with("us") {
        let num_part = &s[..s.len() - 2];
        if num_part.is_empty() {
            return Err("duration must have a numeric value before the unit".to_string());
        }
        let micros: u64 = num_part
            .parse()
            .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;
        return Ok(Duration::from_micros(micros));
    }

    if s.ends_with("ns") {
        let num_part = &s[..s.len() - 2];
        if num_part.is_empty() {
            return Err("duration must have a numeric value before the unit".to_string());
        }
        let nanos: u64 = num_part
            .parse()
            .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;
        return Ok(Duration::from_nanos(nanos));
    }

    // Single character units (s)
    let last_char = s.chars().last().unwrap();

    if last_char != 's' {
        return Err(format!(
            "invalid duration unit '{}': supported units are ns, us, ms, s",
            last_char
        ));
    }

    let num_part = &s[..s.len() - 1];

    if num_part.is_empty() {
        return Err("duration must have a numeric value before the unit".to_string());
    }

    let seconds: u64 = num_part
        .parse()
        .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;

    Ok(Duration::from_secs(seconds))
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    if s.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    let last_char = s.chars().last().unwrap();

    // Only accept 's' as the unit
    if last_char != 's' {
        return Err(format!(
            "invalid duration unit '{}': only 's' (seconds) is supported",
            last_char
        ));
    }

    let num_part = &s[..s.len() - 1];

    if num_part.is_empty() {
        return Err("duration must have a numeric value before the unit".to_string());
    }

    let seconds: u64 = num_part
        .parse()
        .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;

    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Clone)]
struct TaskExecution {
    ideal_time: Instant,
    actual_time: Instant,
    delta_micros: i64, // positive = late, negative = early
    window_before: Duration,
    window_after: Duration,
}

#[derive(Debug)]
enum ExecutionCategory {
    Early,  // Before window start
    OnTime, // Within window
    Late,   // After window end
}

impl TaskExecution {
    fn category(&self) -> ExecutionCategory {
        let window_start = self
            .ideal_time
            .checked_sub(self.window_before)
            .unwrap_or(self.ideal_time);
        let window_end = self
            .ideal_time
            .checked_add(self.window_after)
            .unwrap_or(self.ideal_time);

        if self.actual_time < window_start {
            ExecutionCategory::Early
        } else if self.actual_time <= window_end {
            ExecutionCategory::OnTime
        } else {
            ExecutionCategory::Late
        }
    }
}

#[derive(Debug)]
struct BenchmarkStats {
    total_executions: usize,
    early_count: usize,
    on_time_count: usize,
    late_count: usize,
    missed_count: usize,

    min_delta_micros: i64,
    max_delta_micros: i64,
    avg_delta_micros: f64,
    stddev_delta_micros: f64,

    min_abs_delta_micros: i64,
    max_abs_delta_micros: i64,
    avg_abs_delta_micros: f64,
}

impl BenchmarkStats {
    fn from_executions(executions: &[TaskExecution], missed_count: usize) -> Self {
        if executions.is_empty() {
            return Self {
                total_executions: 0,
                early_count: 0,
                on_time_count: 0,
                late_count: 0,
                missed_count,
                min_delta_micros: 0,
                max_delta_micros: 0,
                avg_delta_micros: 0.0,
                stddev_delta_micros: 0.0,
                min_abs_delta_micros: 0,
                max_abs_delta_micros: 0,
                avg_abs_delta_micros: 0.0,
            };
        }

        let mut early_count = 0;
        let mut on_time_count = 0;
        let mut late_count = 0;

        for exec in executions {
            match exec.category() {
                ExecutionCategory::Early => early_count += 1,
                ExecutionCategory::OnTime => on_time_count += 1,
                ExecutionCategory::Late => late_count += 1,
            }
        }

        let deltas: Vec<i64> = executions.iter().map(|e| e.delta_micros).collect();
        let abs_deltas: Vec<i64> = executions.iter().map(|e| e.delta_micros.abs()).collect();

        let min_delta = *deltas.iter().min().unwrap();
        let max_delta = *deltas.iter().max().unwrap();
        let sum_delta: i64 = deltas.iter().sum();
        let avg_delta = sum_delta as f64 / deltas.len() as f64;

        let variance = deltas
            .iter()
            .map(|&d| {
                let diff = d as f64 - avg_delta;
                diff * diff
            })
            .sum::<f64>()
            / deltas.len() as f64;
        let stddev = variance.sqrt();

        let min_abs_delta = *abs_deltas.iter().min().unwrap();
        let max_abs_delta = *abs_deltas.iter().max().unwrap();
        let sum_abs_delta: i64 = abs_deltas.iter().sum();
        let avg_abs_delta = sum_abs_delta as f64 / abs_deltas.len() as f64;

        Self {
            total_executions: executions.len(),
            early_count,
            on_time_count,
            late_count,
            missed_count,
            min_delta_micros: min_delta,
            max_delta_micros: max_delta,
            avg_delta_micros: avg_delta,
            stddev_delta_micros: stddev,
            min_abs_delta_micros: min_abs_delta,
            max_abs_delta_micros: max_abs_delta,
            avg_abs_delta_micros: avg_abs_delta,
        }
    }

    fn print(&self, task_name: &str, period: Duration) {
        println!("\n╔══════════════════════════════════════════════════════════════╗");
        println!("║ Benchmark Results: {:<42} ║", task_name);
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Period: {:>50} ║", format!("{:?}", period));
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Execution Summary                                            ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║   Total Executions:  {:>40} ║", self.total_executions);
        println!(
            "║   Early:             {:>40} ║",
            format!(
                "{} ({:.1}%)",
                self.early_count,
                self.early_count as f64 / self.total_executions as f64 * 100.0
            )
        );
        println!(
            "║   On-Time:           {:>40} ║",
            format!(
                "{} ({:.1}%)",
                self.on_time_count,
                self.on_time_count as f64 / self.total_executions as f64 * 100.0
            )
        );
        println!(
            "║   Late:              {:>40} ║",
            format!(
                "{} ({:.1}%)",
                self.late_count,
                self.late_count as f64 / self.total_executions as f64 * 100.0
            )
        );
        println!("║   Missed:            {:>40} ║", self.missed_count);
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Timing Accuracy (signed, + = late, - = early)               ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║   Min Delta:         {:>40} ║",
            format!("{:+.1} μs", self.min_delta_micros as f64)
        );
        println!(
            "║   Max Delta:         {:>40} ║",
            format!("{:+.1} μs", self.max_delta_micros as f64)
        );
        println!(
            "║   Avg Delta:         {:>40} ║",
            format!("{:+.1} μs", self.avg_delta_micros)
        );
        println!(
            "║   Std Dev:           {:>40} ║",
            format!("{:.1} μs", self.stddev_delta_micros)
        );
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Timing Accuracy (absolute deviation from ideal)             ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║   Min Abs Delta:     {:>40} ║",
            format!("{:.1} μs", self.min_abs_delta_micros as f64)
        );
        println!(
            "║   Max Abs Delta:     {:>40} ║",
            format!("{:.1} μs", self.max_abs_delta_micros as f64)
        );
        println!(
            "║   Avg Abs Delta:     {:>40} ║",
            format!("{:.1} μs", self.avg_abs_delta_micros)
        );
        println!("╚══════════════════════════════════════════════════════════════╝");
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let benchmark_duration = parse_duration(&args.duration).unwrap_or_else(|e| {
        eprintln!("Error: invalid duration '{}': {}", args.duration, e);
        std::process::exit(1);
    });

    let min_period = parse_duration_with_units(&args.min_period).unwrap_or_else(|e| {
        eprintln!("Error: invalid min-period '{}': {}", args.min_period, e);
        std::process::exit(1);
    });

    let max_period = parse_duration_with_units(&args.max_period).unwrap_or_else(|e| {
        eprintln!("Error: invalid max-period '{}': {}", args.max_period, e);
        std::process::exit(1);
    });

    let task_period = parse_duration_with_units(&args.task_period).unwrap_or_else(|e| {
        eprintln!("Error: invalid task-period '{}': {}", args.task_period, e);
        std::process::exit(1);
    });

    let window_before = Duration::from_millis(50);
    let window_after = Duration::from_millis(50);

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║          Syncopate Scheduler Benchmark                       ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ Running {} benchmark                                        ║",
        format!("{:?}", benchmark_duration)
    );
    println!(
        "║ Scheduler: min={:?}, max={:?}                                 ║",
        min_period, max_period
    );
    println!(
        "║ Task: period={:?}                                             ║",
        task_period
    );
    println!("║ Measuring timing accuracy, drift, and jitter                ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Shared storage for execution data
    let executions = Arc::new(Mutex::new(Vec::new()));
    let missed_count = Arc::new(Mutex::new(0usize));

    let executions_clone = executions.clone();
    let missed_count_clone = missed_count.clone();

    // Build scheduler
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(min_period)
        .max_period(max_period)
        .build();

    let benchmark_start = Instant::now();

    // Clone handle for the task adder
    let handle_clone = handle.clone();
    let task_period_clone = task_period;
    let window_before_clone = window_before;
    let window_after_clone = window_after;

    // Spawn a task to add the benchmark task after scheduler starts
    tokio::spawn(async move {
        // Wait for scheduler to start first poll
        tokio::time::sleep(Duration::from_millis(50)).await;

        println!(
            "Adding task: period={:?}, window_before={:?}, window_after={:?}",
            task_period_clone, window_before_clone, window_after_clone
        );

        // Use spawn_blocking because add_task uses blocking crossbeam channels
        let task_id = tokio::task::spawn_blocking(move || {
            handle_clone.add_task(TaskConfig {
                task_type: TaskType::Periodic {
                    period: task_period_clone,
                    window_before: window_before_clone,
                    window_after: window_after_clone,
                },
                priority: 0,
                name: Some("benchmark_1hz".into()),
            })
        })
        .await
        .expect("Task panicked")
        .expect("Failed to add task");

        println!("Task {:?} scheduled", task_id);
        println!("Starting benchmark...\n");
    });

    // Spawn scheduler loop
    let scheduler_handle = tokio::spawn(async move {
        let mut last_report = Instant::now();
        let report_interval = Duration::from_secs(1);
        let mut _iteration = 0;

        loop {
            let plan = scheduler.poll();

            // Process due tasks
            if !plan.due_tasks.is_empty() {
                let actual_time = Instant::now();

                for task in &plan.due_tasks {
                    let delta_micros =
                        actual_time.duration_since(task.ideal_time).as_micros() as i64;

                    let exec = TaskExecution {
                        ideal_time: task.ideal_time,
                        actual_time,
                        delta_micros,
                        window_before,
                        window_after,
                    };

                    executions_clone.lock().unwrap().push(exec);
                }

                let completed: Vec<_> = plan.due_tasks.iter().map(|t| t.id).collect();
                scheduler.mark_completed(&completed);
            }

            // Process missed tasks
            if !plan.missed_tasks.is_empty() {
                let mut missed = missed_count_clone.lock().unwrap();
                *missed += plan.missed_tasks.len();
            }

            // Periodic progress report
            if last_report.elapsed() >= report_interval {
                let elapsed = benchmark_start.elapsed();
                let execs = executions_clone.lock().unwrap();
                let missed = *missed_count_clone.lock().unwrap();
                println!(
                    "[{:>5.1}s] Executions: {:>3}  Missed: {:>2}  Idle: {:>8.1}ms",
                    elapsed.as_secs_f64(),
                    execs.len(),
                    missed,
                    plan.idle_duration.as_secs_f64() * 1000.0
                );
                last_report = Instant::now();
            }

            // Check if benchmark is complete
            if benchmark_start.elapsed() >= benchmark_duration {
                println!(
                    "\n[{:>5.1}s] Benchmark complete!",
                    benchmark_start.elapsed().as_secs_f64()
                );
                break;
            }

            // Sleep for idle duration (capped to allow command processing)
            // Note: We use crossbeam channels which aren't async-aware,
            // so we need to wake up periodically to check for new commands
            if plan.idle_duration > Duration::ZERO {
                let sleep_duration = plan.idle_duration.min(Duration::from_millis(100));
                tokio::time::sleep(sleep_duration).await;
            }

            _iteration += 1;
        }
    });

    // Wait for scheduler to complete
    scheduler_handle.await.unwrap();

    // Generate and print statistics
    let executions = executions.lock().unwrap();
    let missed_count = *missed_count.lock().unwrap();

    let stats = BenchmarkStats::from_executions(&executions, missed_count);
    stats.print("1 Hz Periodic Task", task_period);

    // Print per-execution details (first 10 and last 10)
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║ Per-Execution Details (first 10 and last 10)                ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ {:>4} │ {:>12} │ {:>10} │ {:>10} ║",
        "#", "Delta (μs)", "Category", "Window"
    );
    println!("╠══════════════════════════════════════════════════════════════╣");

    let show_count = 10.min(executions.len());

    // First 10
    for (i, exec) in executions.iter().take(show_count).enumerate() {
        let category = match exec.category() {
            ExecutionCategory::Early => "Early",
            ExecutionCategory::OnTime => "On-Time",
            ExecutionCategory::Late => "Late",
        };

        println!(
            "║ {:>4} │ {:>+12.1} │ {:>10} │ [{:>3},{:>+4}] ║",
            i + 1,
            exec.delta_micros as f64,
            category,
            -(exec.window_before.as_micros() as i64),
            exec.window_after.as_micros() as i64,
        );
    }

    // Show ellipsis if there are more
    if executions.len() > 2 * show_count {
        println!(
            "║ {:>4} │ {:>12} │ {:>10} │ {:>10} ║",
            "...", "...", "...", "..."
        );
    }

    // Last 10
    if executions.len() > show_count {
        let start_idx = executions.len().saturating_sub(show_count);
        for (idx, exec) in executions.iter().enumerate().skip(start_idx) {
            let category = match exec.category() {
                ExecutionCategory::Early => "Early",
                ExecutionCategory::OnTime => "On-Time",
                ExecutionCategory::Late => "Late",
            };

            println!(
                "║ {:>4} │ {:>+12.1} │ {:>10} │ [{:>3},{:>+4}] ║",
                idx + 1,
                exec.delta_micros as f64,
                category,
                -(exec.window_before.as_micros() as i64),
                exec.window_after.as_micros() as i64,
            );
        }
    }

    println!("╚══════════════════════════════════════════════════════════════╝");

    // Calculate and display drift
    if executions.len() >= 2 {
        let first = &executions[0];
        let last = &executions[executions.len() - 1];

        let expected_executions =
            (benchmark_duration.as_secs_f64() / task_period.as_secs_f64()).floor() as usize;
        let actual_executions = executions.len();

        let expected_duration = task_period * (actual_executions - 1) as u32;
        let actual_duration = last.actual_time.duration_since(first.actual_time);
        let cumulative_drift =
            actual_duration.as_micros() as i64 - expected_duration.as_micros() as i64;

        // Find representative baseline using median execution
        // This avoids skew from initial warm-up delays
        let mut all_deltas: Vec<(usize, i64)> = executions
            .iter()
            .enumerate()
            .map(|(i, exec)| {
                let delta = exec.actual_time.duration_since(exec.ideal_time).as_micros() as i64;
                (i, delta)
            })
            .collect();
        all_deltas.sort_by_key(|(_, delta)| *delta);
        let median_idx = all_deltas[all_deltas.len() / 2].0;
        let baseline_exec = &executions[median_idx];
        let baseline_ideal = baseline_exec.ideal_time;

        // Calculate per-execution drift from corrected baseline
        let mut drifts: Vec<i64> = Vec::new();
        let mut drifted_tasks: Vec<(usize, i64)> = Vec::new();
        let drift_threshold = 10; // Report tasks with >10μs drift

        for (i, exec) in executions.iter().enumerate() {
            let periods_from_baseline = i as i64 - median_idx as i64;
            let expected_time = if periods_from_baseline >= 0 {
                baseline_ideal + task_period * periods_from_baseline as u32
            } else {
                baseline_ideal - task_period * (-periods_from_baseline) as u32
            };
            let drift = exec.actual_time.duration_since(expected_time).as_micros() as i64;
            drifts.push(drift);

            // Track drifted tasks (those with significant positive or negative drift)
            if drift.abs() > drift_threshold {
                drifted_tasks.push((i + 1, drift)); // +1 for 1-based indexing
            }
        }

        // Sort drifted tasks by absolute drift (worst first)
        drifted_tasks.sort_by_key(|(_, drift)| -drift.abs());

        drifts.sort_unstable();

        let min_drift = drifts[0];
        let max_drift = drifts[drifts.len() - 1];
        let avg_drift = drifts.iter().sum::<i64>() as f64 / drifts.len() as f64;
        let p50_idx = drifts.len() / 2;
        let p95_idx = ((drifts.len() as f64) * 0.95).ceil() as usize - 1;
        let p99_idx = ((drifts.len() as f64) * 0.99).ceil() as usize - 1;
        let p50_drift = drifts[p50_idx];
        let p95_drift = drifts[p95_idx.min(drifts.len() - 1)];
        let p99_drift = drifts[p99_idx.min(drifts.len() - 1)];

        let warm_up_drift = all_deltas[0].1 - all_deltas[all_deltas.len() / 2].1;

        println!("\n╔══════════════════════════════════════════════════════════════╗");
        println!("║ Drift Analysis (deviation from ideal timing)                ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║   Expected Executions:  {:>37} ║", expected_executions);
        println!("║   Actual Executions:    {:>37} ║", actual_executions);
        println!(
            "║   Expected Duration:    {:>37} ║",
            format!("{:.6}s", expected_duration.as_secs_f64())
        );
        println!(
            "║   Actual Duration:      {:>37} ║",
            format!("{:.6}s", actual_duration.as_secs_f64())
        );
        println!(
            "║   Cumulative Drift:     {:>37} ║",
            format!("{:+.1} μs", cumulative_drift as f64)
        );
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Per-Execution Drift Statistics                              ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║   Min:                  {:>37} ║",
            format!("{:+.1} μs", min_drift as f64)
        );
        println!(
            "║   Max:                  {:>37} ║",
            format!("{:+.1} μs", max_drift as f64)
        );
        println!(
            "║   Average:              {:>37} ║",
            format!("{:+.1} μs", avg_drift)
        );
        println!(
            "║   P50:                  {:>37} ║",
            format!("{:+.1} μs", p50_drift as f64)
        );
        println!(
            "║   P95:                  {:>37} ║",
            format!("{:+.1} μs", p95_drift as f64)
        );
        println!(
            "║   P99:                  {:>37} ║",
            format!("{:+.1} μs", p99_drift as f64)
        );
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║ Warm-up Compensation                                        ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║   Baseline: Execution #{:<36} ║", median_idx + 1);
        println!(
            "║   Initial Delay:        {:>37} ║",
            format!("{:+.1} μs", warm_up_drift as f64)
        );

        // Print drifted tasks if any
        if !drifted_tasks.is_empty() {
            println!("╠══════════════════════════════════════════════════════════════╣");
            println!(
                "║ Drifted Tasks (>{}μs deviation)                               ║",
                drift_threshold
            );
            println!("╠══════════════════════════════════════════════════════════════╣");
            println!(
                "║ {:>10} │ {:>12} │ {:>28} ║",
                "Execution", "Drift", "Notes"
            );
            println!("╠══════════════════════════════════════════════════════════════╣");

            let show_count = 10.min(drifted_tasks.len());
            for (_idx, (exec_num, drift)) in drifted_tasks.iter().take(show_count).enumerate() {
                let notes = if *exec_num <= 100 {
                    "warm-up"
                } else {
                    "anomaly"
                };
                println!(
                    "║ {:>10} │ {:>+11.1}μs │ {:>28} ║",
                    exec_num, *drift as f64, notes
                );
            }

            if drifted_tasks.len() > show_count {
                println!(
                    "║ {:>10} │ {:>12} │ {:>28} ║",
                    format!("+{} more", drifted_tasks.len() - show_count),
                    "...",
                    ""
                );
            }
        }

        println!("╚══════════════════════════════════════════════════════════════╝");
    }
}
