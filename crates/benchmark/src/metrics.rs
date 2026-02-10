use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BenchmarkResults {
    pub total_executions: usize,
    pub early_count: usize,
    pub on_time_count: usize,
    pub late_count: usize,
    pub missed_count: usize,

    // Baseline offset (median raw delta from ideal)
    pub baseline_delta: i64,

    // Corrected (baseline-subtracted) jitter stats
    pub min_drift: f64,
    pub max_drift: f64,
    pub avg_drift: f64,
    pub stddev_drift: f64,

    // Drift percentiles (after baseline correction)
    pub p50_drift: f64,
    pub p95_drift: f64,
    pub p99_drift: f64,

    // Expected vs actual metrics
    pub expected_executions: usize,
    pub expected_duration_secs: f64,
    pub actual_duration_secs: f64,

    // Resource usage
    pub cpu_time: Duration,
    pub memory_kb: u64,        // Peak memory usage in KB
    pub context_switches: u64, // Number of context switches

    // Scheduler-specific metrics
    pub scheduler_overhead_percent: f64, // Time in scheduler / total time
    pub coalescing_ratio: f64,           // Expected wakeups / actual wakeups
    pub wakeup_count: u64,               // Actual scheduler wakeup count
    pub avg_tasks_per_wakeup: f64,       // Task executions / wakeups
    pub avg_task_execution_us: f64,      // Average time spent executing task callbacks

    // First and last executions for display
    pub first_executions: Vec<(usize, i64, String)>, // (execution_num, offset, category)
    pub last_executions: Vec<(usize, i64, String)>,

    // Top outliers (limited to 10)
    pub top_outliers: Vec<(usize, i64, String, String)>, // (execution_num, drift, category, notes)
}

impl BenchmarkResults {
    #[allow(clippy::too_many_arguments)]
    pub fn from_timestamps(
        timestamps: Vec<(usize, std::time::Instant, std::time::Instant)>, // (timer_id, actual, ideal)
        missed: usize,
        cpu_time: Duration,
        memory_kb: u64,
        context_switches: u64,
        period: Duration,
        benchmark_duration: Duration,
        window_before: Duration,
        window_after: Duration,
        wakeup_count: u64,
        scheduler_overhead: Duration,
        task_execution_duration: Duration,
    ) -> Self {
        if timestamps.is_empty() {
            return Self {
                total_executions: 0,
                early_count: 0,
                on_time_count: 0,
                late_count: 0,
                missed_count: missed,
                baseline_delta: 0,
                min_drift: 0.0,
                max_drift: 0.0,
                avg_drift: 0.0,
                stddev_drift: 0.0,
                p50_drift: 0.0,
                p95_drift: 0.0,
                p99_drift: 0.0,
                expected_executions: 0,
                expected_duration_secs: 0.0,
                actual_duration_secs: 0.0,
                cpu_time,
                memory_kb,
                context_switches,
                scheduler_overhead_percent: 0.0,
                coalescing_ratio: 0.0,
                wakeup_count: 0,
                avg_tasks_per_wakeup: 0.0,
                avg_task_execution_us: 0.0,
                first_executions: Vec::new(),
                last_executions: Vec::new(),
                top_outliers: Vec::new(),
            };
        }

        let window_before_micros = window_before.as_micros() as i64;
        let window_after_micros = window_after.as_micros() as i64;

        // Store first and last executions
        let mut first_executions: Vec<(usize, i64, String)> = Vec::new();
        let mut last_executions: Vec<(usize, i64, String)> = Vec::new();

        // Calculate raw deltas and categorize
        let mut early_count = 0;
        let mut on_time_count = 0;
        let mut late_count = 0;

        let deltas: Vec<i64> = timestamps
            .iter()
            .enumerate()
            .map(|(idx, (_id, actual, ideal))| {
                let delta_micros = if *actual >= *ideal {
                    actual.duration_since(*ideal).as_micros() as i64
                } else {
                    -(ideal.duration_since(*actual).as_micros() as i64)
                };

                // Categorize based on window
                let category = if delta_micros < -window_before_micros {
                    early_count += 1;
                    "Early"
                } else if delta_micros <= window_after_micros {
                    on_time_count += 1;
                    "On-Time"
                } else {
                    late_count += 1;
                    "Late"
                };

                // Store first 10 executions (raw deltas for display)
                if idx < 10 {
                    first_executions.push((idx + 1, delta_micros, category.to_string()));
                }

                // Store last 10 executions (will be filled later)
                // We store all and filter later

                delta_micros
            })
            .collect();

        // Store last 10 executions
        let total = timestamps.len();
        let start_idx = total.saturating_sub(10);
        for (idx, (_id, actual, ideal)) in timestamps.iter().enumerate().skip(start_idx) {
            let delta_micros = if *actual >= *ideal {
                actual.duration_since(*ideal).as_micros() as i64
            } else {
                -(ideal.duration_since(*actual).as_micros() as i64)
            };

            let category = if delta_micros < -window_before_micros {
                "Early"
            } else if delta_micros <= window_after_micros {
                "On-Time"
            } else {
                "Late"
            };

            last_executions.push((idx + 1, delta_micros, category.to_string()));
        }

        // Find representative baseline using median execution (to avoid warm-up skew)
        let mut all_deltas: Vec<(usize, i64)> =
            deltas.iter().enumerate().map(|(i, &d)| (i, d)).collect();
        all_deltas.sort_by_key(|(_, delta)| *delta);
        let median_idx = all_deltas.len() / 2;
        let baseline_delta = all_deltas[median_idx].1;

        // Compute corrected drifts (unsorted) for outlier detection
        let corrected_drifts_unsorted: Vec<f64> = deltas
            .iter()
            .map(|&d| (d - baseline_delta) as f64)
            .collect();

        // Sort a copy for percentile calculation
        let mut corrected_drifts_sorted = corrected_drifts_unsorted.clone();
        corrected_drifts_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let min_drift = corrected_drifts_sorted[0];
        let max_drift = corrected_drifts_sorted[corrected_drifts_sorted.len() - 1];
        let avg_drift =
            corrected_drifts_sorted.iter().sum::<f64>() / corrected_drifts_sorted.len() as f64;

        // Calculate variance and stddev
        let variance = corrected_drifts_sorted
            .iter()
            .map(|&d| {
                let diff = d - avg_drift;
                diff * diff
            })
            .sum::<f64>()
            / corrected_drifts_sorted.len() as f64;
        let stddev_drift = variance.sqrt();

        // Calculate percentiles from sorted data
        let p50_idx = corrected_drifts_sorted.len() / 2;
        let p95_idx = ((corrected_drifts_sorted.len() as f64) * 0.95).ceil() as usize - 1;
        let p99_idx = ((corrected_drifts_sorted.len() as f64) * 0.99).ceil() as usize - 1;
        let p50_drift = corrected_drifts_sorted[p50_idx.min(corrected_drifts_sorted.len() - 1)];
        let p95_drift = corrected_drifts_sorted[p95_idx.min(corrected_drifts_sorted.len() - 1)];
        let p99_drift = corrected_drifts_sorted[p99_idx.min(corrected_drifts_sorted.len() - 1)];

        // Calculate expected metrics
        let expected_executions =
            (benchmark_duration.as_secs_f64() / period.as_secs_f64()) as usize;
        let expected_duration_secs = period.as_secs_f64() * (timestamps.len() as f64 - 1.0);

        // Get actual duration from first to last execution
        let actual_duration_secs = if timestamps.len() >= 2 {
            let first = timestamps.first().unwrap();
            let last = timestamps.last().unwrap();
            last.1.duration_since(first.1).as_secs_f64()
        } else {
            0.0
        };

        // Build outliers from unsorted data (correct index mapping)
        let warm_up_threshold = (deltas.len() as f64 * 0.05).ceil() as usize;
        let mut drifted_tasks: Vec<(usize, i64, String, String)> = Vec::new();
        for (idx, (&raw_delta, &corrected)) in deltas
            .iter()
            .zip(corrected_drifts_unsorted.iter())
            .enumerate()
        {
            let category = if raw_delta < -window_before_micros {
                "Early"
            } else if raw_delta <= window_after_micros {
                "On-Time"
            } else {
                "Late"
            };

            let notes = if corrected.abs() > 10.0 {
                if idx < warm_up_threshold {
                    "warm-up"
                } else {
                    "anomaly"
                }
            } else {
                ""
            };

            drifted_tasks.push((
                idx + 1,
                corrected as i64,
                category.to_string(),
                notes.to_string(),
            ));
        }

        // Sort by absolute corrected drift and take top 10 with >10μs deviation
        drifted_tasks.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()));
        let top_outliers: Vec<_> = drifted_tasks
            .into_iter()
            .filter(|(_, drift, _, _)| drift.abs() > 10)
            .take(10)
            .collect();

        // Calculate scheduler-specific metrics
        let total_time_secs = benchmark_duration.as_secs_f64();
        let scheduler_overhead_percent = if total_time_secs > 0.0 {
            (scheduler_overhead.as_secs_f64() / total_time_secs) * 100.0
        } else {
            0.0
        };

        // Calculate expected wakeups: each timer should wake up duration/period times
        let expected_wakeups_per_timer =
            (benchmark_duration.as_secs_f64() / period.as_secs_f64()) as u64;
        let num_timers = if timestamps.is_empty() {
            1
        } else {
            timestamps.iter().map(|(id, _, _)| *id).max().unwrap_or(0) + 1
        };
        let expected_wakeups = expected_wakeups_per_timer * num_timers as u64;

        let coalescing_ratio = if wakeup_count > 0 {
            expected_wakeups as f64 / wakeup_count as f64
        } else {
            0.0
        };

        let avg_tasks_per_wakeup = if wakeup_count > 0 {
            timestamps.len() as f64 / wakeup_count as f64
        } else {
            0.0
        };

        let avg_task_execution_us = if !timestamps.is_empty() {
            task_execution_duration.as_micros() as f64 / timestamps.len() as f64
        } else {
            0.0
        };

        Self {
            total_executions: timestamps.len(),
            early_count,
            on_time_count,
            late_count,
            missed_count: missed,
            baseline_delta,
            min_drift,
            max_drift,
            avg_drift,
            stddev_drift,
            p50_drift,
            p95_drift,
            p99_drift,
            expected_executions,
            expected_duration_secs,
            actual_duration_secs,
            cpu_time,
            memory_kb,
            context_switches,
            scheduler_overhead_percent,
            coalescing_ratio,
            wakeup_count,
            avg_tasks_per_wakeup,
            avg_task_execution_us,
            first_executions,
            last_executions,
            top_outliers,
        }
    }

    pub fn print(
        &self,
        name: &str,
        period: Duration,
        window_before: Duration,
        window_after: Duration,
    ) {
        // Helper to create a two-column key-value table with a header
        fn kv_table(title: &str, rows: Vec<(&str, String)>) -> Table {
            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(vec![title, ""]);
            for (label, value) in rows {
                table.add_row(vec![
                    Cell::new(label),
                    Cell::new(value).set_alignment(CellAlignment::Right),
                ]);
            }
            table
        }

        let pct = |count: usize| -> String {
            if self.total_executions > 0 {
                format!(
                    "{} ({:.1}%)",
                    count,
                    count as f64 / self.total_executions as f64 * 100.0
                )
            } else {
                format!("{} (0.0%)", count)
            }
        };

        // Main results
        println!(
            "\n{}",
            kv_table(
                &format!("Benchmark Results: {}", name),
                vec![("Period", format!("{:?}", period)),],
            )
        );

        // Execution Summary
        println!(
            "\n{}",
            kv_table(
                "Execution Summary",
                vec![
                    ("Total Executions", format!("{}", self.total_executions)),
                    ("Early", pct(self.early_count)),
                    ("On-Time", pct(self.on_time_count)),
                    ("Late", pct(self.late_count)),
                    ("Missed", format!("{}", self.missed_count)),
                ],
            )
        );

        // Timing Jitter (baseline-corrected)
        println!(
            "\n{}",
            kv_table(
                "Timing Jitter (baseline-corrected)",
                vec![
                    ("Min", format!("{:+.1} μs", self.min_drift)),
                    ("Max", format!("{:+.1} μs", self.max_drift)),
                    ("Average", format!("{:+.1} μs", self.avg_drift)),
                    ("Std Dev", format!("{:.1} μs", self.stddev_drift)),
                    ("P50", format!("{:+.1} μs", self.p50_drift)),
                    ("P95", format!("{:+.1} μs", self.p95_drift)),
                    ("P99", format!("{:+.1} μs", self.p99_drift)),
                ],
            )
        );

        // Resource Usage
        println!(
            "\n{}",
            kv_table(
                "Resource Usage",
                vec![
                    ("CPU Time", format!("{:.3?}", self.cpu_time)),
                    (
                        "Memory",
                        if self.memory_kb >= 1024 {
                            format!("{:.2} MB", self.memory_kb as f64 / 1024.0)
                        } else {
                            format!("{} KB", self.memory_kb)
                        }
                    ),
                    ("Context Switches", format!("{}", self.context_switches)),
                ],
            )
        );

        // Scheduler Metrics
        let coalescing = if self.coalescing_ratio >= 1.0 {
            format!("{:.2}x (fewer wakeups)", self.coalescing_ratio)
        } else {
            format!("{:.2}x (no coalescing)", self.coalescing_ratio)
        };
        println!(
            "\n{}",
            kv_table(
                "Scheduler Metrics",
                vec![
                    (
                        "Scheduler Overhead",
                        format!("{:.2}%", self.scheduler_overhead_percent)
                    ),
                    ("Wakeup Count", format!("{}", self.wakeup_count)),
                    ("Coalescing Ratio", coalescing),
                    (
                        "Avg Tasks/Wakeup",
                        format!("{:.2}", self.avg_tasks_per_wakeup)
                    ),
                    (
                        "Avg Task Exec Time",
                        format!("{:.2} μs", self.avg_task_execution_us)
                    ),
                ],
            )
        );

        // Per-Execution Details
        if !self.first_executions.is_empty() {
            let window_before_micros = window_before.as_micros() as i64;
            let window_after_micros = window_after.as_micros() as i64;
            let window_str = format!("[{:+},{:+}]", -window_before_micros, window_after_micros);

            let mut details = Table::new();
            details.set_content_arrangement(ContentArrangement::Dynamic);
            details.set_header(vec!["#", "Offset (μs)", "Category", "Window"]);

            for (exec_num, delta, category) in &self.first_executions {
                details.add_row(vec![
                    Cell::new(exec_num).set_alignment(CellAlignment::Right),
                    Cell::new(format!("{:+.1}", *delta as f64)).set_alignment(CellAlignment::Right),
                    Cell::new(category).set_alignment(CellAlignment::Right),
                    Cell::new(&window_str),
                ]);
            }

            if self.total_executions > 20 {
                details.add_row(vec!["...", "...", "...", "..."]);

                for (exec_num, delta, category) in &self.last_executions {
                    details.add_row(vec![
                        Cell::new(exec_num).set_alignment(CellAlignment::Right),
                        Cell::new(format!("{:+.1}", *delta as f64))
                            .set_alignment(CellAlignment::Right),
                        Cell::new(category).set_alignment(CellAlignment::Right),
                        Cell::new(&window_str),
                    ]);
                }
            }

            println!("\n{}", details);
        }

        // Drift Analysis
        let cumulative_drift =
            (self.actual_duration_secs - self.expected_duration_secs) * 1_000_000.0;
        let baseline_note = if self.baseline_delta < 0 {
            format!(
                "tasks fire ~{}μs early within window",
                self.baseline_delta.abs()
            )
        } else if self.baseline_delta > 0 {
            format!("tasks fire ~{}μs late within window", self.baseline_delta)
        } else {
            "tasks fire on ideal time".to_string()
        };
        println!(
            "\n{}",
            kv_table(
                "Drift Analysis (deviation from ideal timing)",
                vec![
                    (
                        "Expected Executions",
                        format!("{}", self.expected_executions)
                    ),
                    ("Actual Executions", format!("{}", self.total_executions)),
                    (
                        "Expected Duration",
                        format!("{:.6}s", self.expected_duration_secs)
                    ),
                    (
                        "Actual Duration",
                        format!("{:.6}s", self.actual_duration_secs)
                    ),
                    ("Cumulative Drift", format!("{:+.1} μs", cumulative_drift)),
                    (
                        "Baseline Offset",
                        format!("{:+} μs ({})", self.baseline_delta, baseline_note)
                    ),
                ],
            )
        );

        // Drifted Tasks (outliers)
        if !self.top_outliers.is_empty() {
            let mut outliers = Table::new();
            outliers.set_content_arrangement(ContentArrangement::Dynamic);
            outliers.set_header(vec!["Drifted Tasks (>10μs deviation)", "", "", ""]);

            // Column headers as first row
            outliers.add_row(vec![
                Cell::new("Execution").set_alignment(CellAlignment::Right),
                Cell::new("Drift").set_alignment(CellAlignment::Right),
                Cell::new("Category").set_alignment(CellAlignment::Right),
                Cell::new("Notes"),
            ]);

            let show_count = 10.min(self.top_outliers.len());
            for (exec_num, drift, category, notes) in self.top_outliers.iter().take(show_count) {
                outliers.add_row(vec![
                    Cell::new(exec_num).set_alignment(CellAlignment::Right),
                    Cell::new(format!("{:+.1}μs", *drift as f64))
                        .set_alignment(CellAlignment::Right),
                    Cell::new(category).set_alignment(CellAlignment::Right),
                    Cell::new(notes),
                ]);
            }

            if self.top_outliers.len() > show_count {
                outliers.add_row(vec![
                    Cell::new(format!("+{} more", self.top_outliers.len() - show_count)),
                    Cell::new("..."),
                    Cell::new(""),
                    Cell::new(""),
                ]);
            }

            println!("\n{}", outliers);
        }
    }
}
