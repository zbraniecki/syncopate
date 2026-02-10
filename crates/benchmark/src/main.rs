use clap::Parser;
use comfy_table::{ContentArrangement, Table};
use std::time::Duration;

mod export;
mod metrics;
mod platform;
mod raw_timers;
mod scenarios;
mod statistics;
mod syncopate_runner;
mod syncopate_std_runner;
mod visualize;

use metrics::BenchmarkResults;

#[derive(Parser)]
#[command(name = "syncopate-benchmark")]
#[command(about = "Cross-platform timer benchmark - native timers vs syncopate")]
struct Args {
    /// Duration to run the benchmark (e.g., 10s)
    #[arg(long, value_name = "DURATION")]
    duration: Option<String>,

    /// Task timer period (default: 1s)
    #[arg(long, value_name = "DURATION", default_value = "1s")]
    task_period: String,

    /// Scheduler minimum period (default: 100us, syncopate only)
    #[arg(long, value_name = "DURATION", default_value = "100us")]
    min_period: String,

    /// Scheduler maximum period (default: 2s, syncopate only)
    #[arg(long, value_name = "DURATION", default_value = "2s")]
    max_period: String,

    /// Number of concurrent timers
    #[arg(long, default_value = "1")]
    timers: usize,

    /// System to benchmark: 'system' (native timer) or 'syncopate' (default: syncopate)
    #[arg(long, value_name = "SYSTEM", default_value = "syncopate")]
    system: String,

    /// Compare system vs syncopate side-by-side
    #[arg(long)]
    compare: bool,

    /// Window before ideal time (default: 50ms)
    #[arg(long, value_name = "DURATION", default_value = "50ms")]
    window_before: String,

    /// Window after ideal time (default: 50ms)
    #[arg(long, value_name = "DURATION", default_value = "50ms")]
    window_after: String,

    /// Use a predefined scenario (light, medium, heavy, extreme, mixed-frequency, burst, coalescing-test)
    #[arg(long, value_name = "NAME")]
    scenario: Option<String>,

    /// Run progressive stress test suite (light → medium → heavy → extreme)
    #[arg(long)]
    stress_test: bool,

    /// Output format: console (default), json, csv
    #[arg(long, value_name = "FORMAT", default_value = "console")]
    output_format: String,

    /// Output file path (for json/csv formats)
    #[arg(long, value_name = "PATH")]
    output_file: Option<String>,

    /// Sleep compensation for OS/runtime overshoot (default: 0us, syncopate only)
    #[arg(long, value_name = "DURATION", default_value = "0us")]
    sleep_compensation: String,

    /// Show ASCII visualizations (histogram, CDF, time-series)
    #[arg(long)]
    visualize: bool,

    /// Compare all timer mechanisms (raw std, raw tokio, syncopate+std, syncopate+tokio)
    #[arg(long)]
    compare_timers: bool,

    /// Sleep mechanism for syncopate: 'std' or 'tokio' (default: tokio)
    #[arg(long, value_name = "MECHANISM", default_value = "tokio")]
    sleep_mechanism: String,
}

fn parse_duration_with_units(s: &str) -> Result<Duration, String> {
    if s.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    // Check for multi-character units (ms, us, ns) first
    if let Some(num_part) = s.strip_suffix("ms") {
        if num_part.is_empty() {
            return Err("duration must have a numeric value before the unit".to_string());
        }
        let millis: u64 = num_part
            .parse()
            .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;
        return Ok(Duration::from_millis(millis));
    }

    if let Some(num_part) = s.strip_suffix("us") {
        if num_part.is_empty() {
            return Err("duration must have a numeric value before the unit".to_string());
        }
        let micros: u64 = num_part
            .parse()
            .map_err(|_| format!("invalid numeric value '{}' in duration", num_part))?;
        return Ok(Duration::from_micros(micros));
    }

    if let Some(num_part) = s.strip_suffix("ns") {
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

#[tokio::main]
async fn main() {
    // Enforce release mode for accurate benchmarks
    if cfg!(debug_assertions) {
        eprintln!("ERROR: Must run in release mode for accurate benchmarks");
        eprintln!("Run with: cargo run --release -p syncopate-benchmark -- ...");
        std::process::exit(1);
    }

    let args = Args::parse();

    // Handle timer comparison
    if args.compare_timers {
        run_timer_comparison(&args).await;
        return;
    }

    // Handle stress test suite
    if args.stress_test {
        run_stress_test_suite(&args).await;
        return;
    }

    // Handle scenario-based execution
    if let Some(ref scenario_name) = args.scenario {
        run_scenario(&args, scenario_name).await;
        return;
    }

    // Handle manual configuration
    let duration_str = args
        .duration
        .as_ref()
        .expect("--duration is required when not using --scenario or --stress-test");

    let duration = parse_duration_with_units(duration_str).unwrap_or_else(|e| {
        eprintln!("Error: invalid duration '{}': {}", duration_str, e);
        std::process::exit(1);
    });

    let task_period = parse_duration_with_units(&args.task_period).unwrap_or_else(|e| {
        eprintln!("Error: invalid task-period '{}': {}", args.task_period, e);
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

    let window_before = parse_duration_with_units(&args.window_before).unwrap_or_else(|e| {
        eprintln!(
            "Error: invalid window-before '{}': {}",
            args.window_before, e
        );
        std::process::exit(1);
    });

    let window_after = parse_duration_with_units(&args.window_after).unwrap_or_else(|e| {
        eprintln!("Error: invalid window-after '{}': {}", args.window_after, e);
        std::process::exit(1);
    });

    let sleep_compensation =
        parse_duration_with_units(&args.sleep_compensation).unwrap_or_else(|e| {
            eprintln!(
                "Error: invalid sleep-compensation '{}': {}",
                args.sleep_compensation, e
            );
            std::process::exit(1);
        });

    // Validate system argument
    let system = args.system.to_lowercase();
    if system != "system" && system != "syncopate" {
        eprintln!(
            "Error: invalid system '{}'. Must be 'system' or 'syncopate'",
            args.system
        );
        std::process::exit(1);
    }

    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Cross-Platform Timer Benchmark", ""])
        .add_row(vec!["Duration:", &format!("{:?}", duration)])
        .add_row(vec!["Task Period:", &format!("{:?}", task_period)])
        .add_row(vec!["Min Period:", &format!("{:?}", min_period)])
        .add_row(vec!["Max Period:", &format!("{:?}", max_period)])
        .add_row(vec!["Timers:", &args.timers.to_string()])
        .add_row(vec![
            "Window:",
            &format!("[-{:?}, +{:?}]", window_before, window_after),
        ]);

    if args.compare {
        table.add_row(vec!["Mode:", "Compare (System vs Syncopate)"]);
    } else {
        table.add_row(vec!["System:", &system]);
    }

    println!("{}", table);
    println!();

    if args.compare {
        // Run both and compare
        println!("Running system (native) timer benchmark...");
        let system_results = platform::run_benchmark(
            duration,
            task_period,
            args.timers,
            window_before,
            window_after,
        )
        .await;

        println!("\nRunning syncopate scheduler benchmark...");
        let syncopate_results = syncopate_runner::run_benchmark(
            duration,
            task_period,
            min_period,
            max_period,
            args.timers,
            window_before,
            window_after,
            sleep_compensation,
        )
        .await;

        // Handle output format
        handle_output(
            &args,
            &system_results,
            Some(&syncopate_results),
            None,
            task_period,
            window_before,
            window_after,
        );
    } else if system == "system" {
        // Run only system benchmark
        println!("Running system (native) timer benchmark...");
        let results = platform::run_benchmark(
            duration,
            task_period,
            args.timers,
            window_before,
            window_after,
        )
        .await;
        handle_output(
            &args,
            &results,
            None,
            None,
            task_period,
            window_before,
            window_after,
        );
    } else {
        // Run only syncopate benchmark (default)
        println!("Running syncopate scheduler benchmark...");
        let results = syncopate_runner::run_benchmark(
            duration,
            task_period,
            min_period,
            max_period,
            args.timers,
            window_before,
            window_after,
            sleep_compensation,
        )
        .await;
        handle_output(
            &args,
            &results,
            None,
            None,
            task_period,
            window_before,
            window_after,
        );
    }
}

async fn run_scenario(args: &Args, scenario_name: &str) {
    let scenario = scenarios::BenchmarkScenario::get(scenario_name).unwrap_or_else(|| {
        eprintln!(
            "Error: unknown scenario '{}'. Available scenarios:",
            scenario_name
        );
        eprintln!("  light, medium, heavy, extreme, mixed-frequency, burst, coalescing-test");
        std::process::exit(1);
    });

    println!("╔════════════════════════════════════════════════╗");
    println!("║ Running Scenario: {:<29}║", scenario.name);
    println!("╠════════════════════════════════════════════════╣");
    println!("║ Description: {:<35}║", scenario.description);
    println!("║ Duration: {:?}{:<36}║", scenario.duration, "");
    println!("║ Total Timers: {:<32}║", scenario.total_timers());
    println!("╚════════════════════════════════════════════════╝");
    println!();

    let min_period = parse_duration_with_units(&args.min_period).unwrap();
    let max_period = parse_duration_with_units(&args.max_period).unwrap();
    let sleep_compensation = parse_duration_with_units(&args.sleep_compensation).unwrap();

    if args.compare {
        // Run system benchmark
        println!("Running system (native) timer benchmark...");
        let mut system_results = None;
        for timer_config in &scenario.timers {
            let results = platform::run_benchmark(
                scenario.duration,
                timer_config.period,
                timer_config.count,
                timer_config.window_before,
                timer_config.window_after,
            )
            .await;
            system_results = Some(results);
        }

        // Run syncopate benchmark
        println!("\nRunning syncopate scheduler benchmark...");
        let mut syncopate_results = None;
        for timer_config in &scenario.timers {
            let results = syncopate_runner::run_benchmark(
                scenario.duration,
                timer_config.period,
                min_period,
                max_period,
                timer_config.count,
                timer_config.window_before,
                timer_config.window_after,
                sleep_compensation,
            )
            .await;
            syncopate_results = Some(results);
        }

        handle_output(
            args,
            system_results.as_ref().unwrap(),
            syncopate_results.as_ref(),
            Some(scenario_name),
            scenario.timers[0].period,
            scenario.timers[0].window_before,
            scenario.timers[0].window_after,
        );
    } else {
        // Run single system
        let system = args.system.to_lowercase();
        println!("Running {} benchmark...", system);

        for timer_config in &scenario.timers {
            let results = if system == "system" {
                platform::run_benchmark(
                    scenario.duration,
                    timer_config.period,
                    timer_config.count,
                    timer_config.window_before,
                    timer_config.window_after,
                )
                .await
            } else {
                syncopate_runner::run_benchmark(
                    scenario.duration,
                    timer_config.period,
                    min_period,
                    max_period,
                    timer_config.count,
                    timer_config.window_before,
                    timer_config.window_after,
                    sleep_compensation,
                )
                .await
            };

            handle_output(
                args,
                &results,
                None,
                Some(scenario_name),
                timer_config.period,
                timer_config.window_before,
                timer_config.window_after,
            );
        }
    }
}

async fn run_stress_test_suite(args: &Args) {
    let scenarios = vec!["light", "medium", "heavy", "extreme"];

    println!("╔════════════════════════════════════════════════╗");
    println!("║ Progressive Stress Test Suite                 ║");
    println!("╠════════════════════════════════════════════════╣");
    println!("║ Running scenarios: light → medium → heavy → extreme");
    println!("╚════════════════════════════════════════════════╝");
    println!();

    for scenario_name in scenarios {
        println!("\n{}", "═".repeat(50));
        run_scenario(args, scenario_name).await;
        println!("{}\n", "═".repeat(50));
    }
}

fn handle_output(
    args: &Args,
    results: &BenchmarkResults,
    syncopate_results: Option<&BenchmarkResults>,
    scenario_name: Option<&str>,
    task_period: Duration,
    window_before: Duration,
    window_after: Duration,
) {
    match args.output_format.as_str() {
        "json" => {
            let output_path = args
                .output_file
                .as_ref()
                .expect("--output-file is required for JSON format");

            let config = export::BenchmarkConfig {
                system: args.system.clone(),
                scenario: scenario_name.map(|s| s.to_string()),
                duration_secs: results.total_executions as u64, // Approximate
                timers: args.timers,
                task_period_us: task_period.as_micros() as u64,
            };

            export::export_json(results, &config, output_path).unwrap_or_else(|e| {
                eprintln!("Error exporting JSON: {}", e);
                std::process::exit(1);
            });

            println!("Results exported to: {}", output_path);
        }
        "csv" => {
            let output_path = args
                .output_file
                .as_ref()
                .expect("--output-file is required for CSV format");

            export::export_csv(results, output_path).unwrap_or_else(|e| {
                eprintln!("Error exporting CSV: {}", e);
                std::process::exit(1);
            });

            println!("Results exported to: {}", output_path);
        }
        _ => {
            // Print results to console
            if let Some(syncopate) = syncopate_results {
                results.print(
                    "System (Native) Timer",
                    task_period,
                    window_before,
                    window_after,
                );
                syncopate.print(
                    "Syncopate Scheduler",
                    task_period,
                    window_before,
                    window_after,
                );

                // Print statistical comparison
                print_statistical_comparison(results, syncopate);

                // Print pass/fail if scenario
                if let Some(scenario) = scenario_name {
                    print_pass_fail_scorecard(syncopate, scenario);
                }
            } else {
                let title = if args.system == "system" {
                    "System (Native) Timer"
                } else {
                    "Syncopate Scheduler"
                };
                results.print(title, task_period, window_before, window_after);

                // Print pass/fail if scenario
                if let Some(scenario) = scenario_name {
                    print_pass_fail_scorecard(results, scenario);
                }
            }

            // Print visualizations if requested
            if args.visualize {
                print_visualizations(results);
            }
        }
    }
}

fn print_statistical_comparison(system: &BenchmarkResults, syncopate: &BenchmarkResults) {
    use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};

    let comparisons = statistics::compare_benchmarks(syncopate, system);

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Performance Comparison",
        "Syncopate",
        "System",
        "Diff",
    ]);

    let mut wins = 0;
    let mut losses = 0;

    for comp in &comparisons {
        let winner_symbol = match comp.winner {
            statistics::Winner::Syncopate => {
                wins += 1;
                "✓"
            }
            statistics::Winner::System => {
                losses += 1;
                "✗"
            }
            statistics::Winner::Tie => "≈",
        };

        let diff_pct = if comp.system_value != 0.0 {
            ((comp.syncopate_value - comp.system_value) / comp.system_value) * 100.0
        } else {
            0.0
        };

        table.add_row(vec![
            Cell::new(format!("{} {}", winner_symbol, comp.metric_name)),
            Cell::new(format!("{:.1}", comp.syncopate_value)).set_alignment(CellAlignment::Right),
            Cell::new(format!("{:.1}", comp.system_value)).set_alignment(CellAlignment::Right),
            Cell::new(format!("{:+.1}%", diff_pct)).set_alignment(CellAlignment::Right),
        ]);
    }

    println!("\n{}", table);

    let ties = comparisons.len() - wins - losses;
    let verdict = if wins > losses {
        format!(
            "Verdict: {} wins, {} losses, {} ties - Syncopate is better",
            wins, losses, ties
        )
    } else if wins == losses {
        format!(
            "Verdict: {} wins, {} losses, {} ties - comparable",
            wins, losses, ties
        )
    } else {
        format!(
            "Verdict: {} wins, {} losses, {} ties - System is better",
            wins, losses, ties
        )
    };
    println!("{}", verdict);
}

fn print_pass_fail_scorecard(results: &BenchmarkResults, scenario_name: &str) {
    use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};

    let criteria = scenarios::AcceptanceCriteria::for_scenario(scenario_name);

    let on_time_percent = (results.on_time_count as f64 / results.total_executions as f64) * 100.0;
    let missed_percent = (results.missed_count as f64 / results.total_executions as f64) * 100.0;
    let cpu_percent = (results.cpu_time.as_secs_f64() / results.actual_duration_secs) * 100.0;

    let avg_drift_pass = results.avg_drift.abs() <= criteria.max_avg_drift_us;
    let p99_drift_pass = results.p99_drift.abs() <= criteria.max_p99_drift_us;
    let on_time_pass = on_time_percent >= criteria.min_on_time_percent;
    let missed_pass = missed_percent <= criteria.max_missed_percent;
    let cpu_pass = cpu_percent <= criteria.max_cpu_percent;

    let all_pass = avg_drift_pass && p99_drift_pass && on_time_pass && missed_pass && cpu_pass;

    let pass_fail = |pass: bool| if pass { "✓" } else { "✗" };

    let title = if all_pass {
        "Benchmark Verdict: PASS ✓"
    } else {
        "Benchmark Verdict: FAIL ✗"
    };

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![title, "Value", "Target", ""]);

    table.add_row(vec![
        Cell::new("Avg Drift"),
        Cell::new(format!("{:.1}μs", results.avg_drift.abs())).set_alignment(CellAlignment::Right),
        Cell::new(format!("< {:.0}μs", criteria.max_avg_drift_us))
            .set_alignment(CellAlignment::Right),
        Cell::new(pass_fail(avg_drift_pass)),
    ]);
    table.add_row(vec![
        Cell::new("P99 Drift"),
        Cell::new(format!("{:.1}μs", results.p99_drift.abs())).set_alignment(CellAlignment::Right),
        Cell::new(format!("< {:.0}μs", criteria.max_p99_drift_us))
            .set_alignment(CellAlignment::Right),
        Cell::new(pass_fail(p99_drift_pass)),
    ]);
    table.add_row(vec![
        Cell::new("On-Time"),
        Cell::new(format!("{:.1}%", on_time_percent)).set_alignment(CellAlignment::Right),
        Cell::new(format!("> {:.0}%", criteria.min_on_time_percent))
            .set_alignment(CellAlignment::Right),
        Cell::new(pass_fail(on_time_pass)),
    ]);
    table.add_row(vec![
        Cell::new("Missed"),
        Cell::new(format!("{:.1}%", missed_percent)).set_alignment(CellAlignment::Right),
        Cell::new(format!("< {:.1}%", criteria.max_missed_percent))
            .set_alignment(CellAlignment::Right),
        Cell::new(pass_fail(missed_pass)),
    ]);
    table.add_row(vec![
        Cell::new("CPU Usage"),
        Cell::new(format!("{:.1}%", cpu_percent)).set_alignment(CellAlignment::Right),
        Cell::new(format!("< {:.0}%", criteria.max_cpu_percent))
            .set_alignment(CellAlignment::Right),
        Cell::new(pass_fail(cpu_pass)),
    ]);

    println!("\n{}", table);
}

fn print_visualizations(results: &BenchmarkResults) {
    // Collect drift values (need to extract from results)
    // For now, use synthetic data based on statistics
    println!(
        "\n{}",
        visualize::render_histogram(
            &[results.avg_drift], // Would need actual per-execution data
            "Drift Distribution",
            "μs"
        )
    );

    println!(
        "\n{}",
        visualize::render_cdf(
            &[
                results.avg_drift,
                results.p50_drift,
                results.p95_drift,
                results.p99_drift
            ],
            "Cumulative Distribution",
            "μs"
        )
    );
}

async fn run_timer_comparison(args: &Args) {
    use comfy_table::{ContentArrangement, Table};

    let duration = parse_duration_with_units(args.duration.as_ref().unwrap_or(&"10s".to_string()))
        .unwrap_or(Duration::from_secs(10));

    let task_period = parse_duration_with_units(&args.task_period).unwrap();
    let min_period = parse_duration_with_units(&args.min_period).unwrap();
    let max_period = parse_duration_with_units(&args.max_period).unwrap();
    let window_before = parse_duration_with_units(&args.window_before).unwrap();
    let window_after = parse_duration_with_units(&args.window_after).unwrap();
    let sleep_compensation = parse_duration_with_units(&args.sleep_compensation).unwrap();

    let mut config_table = Table::new();
    config_table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Timer Implementation Comparison", ""])
        .add_row(vec!["Duration:", &format!("{:?}", duration)])
        .add_row(vec!["Period:", &format!("{:?}", task_period)])
        .add_row(vec![
            "Window:",
            &format!("[-{:?}, +{:?}]", window_before, window_after),
        ]);

    println!("{}", config_table);
    println!();

    // Run all four benchmarks
    println!("Running 4 timer implementations...\n");

    // 1. Raw std::thread::sleep
    let raw_std = tokio::task::spawn_blocking(move || {
        raw_timers::run_std_sleep_benchmark(duration, task_period, window_before, window_after)
    })
    .await
    .unwrap();

    // 2. Raw tokio::time::sleep
    let raw_tokio =
        raw_timers::run_tokio_sleep_benchmark(duration, task_period, window_before, window_after)
            .await;

    // 3. Syncopate with std::thread::sleep
    let syncopate_std = tokio::task::spawn_blocking(move || {
        syncopate_std_runner::run_benchmark(
            duration,
            task_period,
            min_period,
            max_period,
            1, // single timer for fair comparison
            window_before,
            window_after,
            sleep_compensation,
        )
    })
    .await
    .unwrap();

    // 4. Syncopate with tokio::time::sleep
    let syncopate_tokio = syncopate_runner::run_benchmark(
        duration,
        task_period,
        min_period,
        max_period,
        1, // single timer for fair comparison
        window_before,
        window_after,
        sleep_compensation,
    )
    .await;

    // Print comparison table
    print_timer_comparison_table(&raw_std, &raw_tokio, &syncopate_std, &syncopate_tokio);
}

fn print_timer_comparison_table(
    raw_std: &BenchmarkResults,
    raw_tokio: &BenchmarkResults,
    syncopate_std: &BenchmarkResults,
    syncopate_tokio: &BenchmarkResults,
) {
    use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Metric",
        "Raw Std",
        "Raw Tokio",
        "Syn+Std",
        "Syn+Tokio",
        "Unit",
    ]);

    // Total Executions
    table.add_row(vec![
        Cell::new("Total Executions"),
        Cell::new(raw_std.total_executions.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(raw_tokio.total_executions.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(syncopate_std.total_executions.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(syncopate_tokio.total_executions.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(""),
    ]);

    // Baseline Delta (raw median overshoot before correction)
    table.add_row(vec![
        Cell::new("Baseline Delta"),
        Cell::new(format!("{:+}", raw_std.baseline_delta)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:+}", raw_tokio.baseline_delta)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:+}", syncopate_std.baseline_delta))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:+}", syncopate_tokio.baseline_delta))
            .set_alignment(CellAlignment::Right),
        Cell::new("μs"),
    ]);

    // Avg Abs Drift
    table.add_row(vec![
        Cell::new("Avg Abs Drift"),
        Cell::new(format!("{:.1}", raw_std.avg_drift.abs())).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", raw_tokio.avg_drift.abs())).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", syncopate_std.avg_drift.abs()))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", syncopate_tokio.avg_drift.abs()))
            .set_alignment(CellAlignment::Right),
        Cell::new("μs"),
    ]);

    // Avg Drift (signed)
    table.add_row(vec![
        Cell::new("Avg Drift (signed)"),
        Cell::new(format!("{:+.1}", raw_std.avg_drift)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:+.1}", raw_tokio.avg_drift)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:+.1}", syncopate_std.avg_drift)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:+.1}", syncopate_tokio.avg_drift)).set_alignment(CellAlignment::Right),
        Cell::new("μs"),
    ]);

    // Std Dev
    table.add_row(vec![
        Cell::new("Std Dev"),
        Cell::new(format!("{:.1}", raw_std.stddev_drift)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", raw_tokio.stddev_drift)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", syncopate_std.stddev_drift)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", syncopate_tokio.stddev_drift))
            .set_alignment(CellAlignment::Right),
        Cell::new("μs"),
    ]);

    // P99 Drift
    table.add_row(vec![
        Cell::new("P99 Drift"),
        Cell::new(format!("{:.1}", raw_std.p99_drift.abs())).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", raw_tokio.p99_drift.abs())).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", syncopate_std.p99_drift.abs()))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.1}", syncopate_tokio.p99_drift.abs()))
            .set_alignment(CellAlignment::Right),
        Cell::new("μs"),
    ]);

    // CPU Usage
    let raw_std_cpu_pct = (raw_std.cpu_time.as_secs_f64() / raw_std.actual_duration_secs) * 100.0;
    let raw_tokio_cpu_pct =
        (raw_tokio.cpu_time.as_secs_f64() / raw_tokio.actual_duration_secs) * 100.0;
    let syncopate_std_cpu_pct =
        (syncopate_std.cpu_time.as_secs_f64() / syncopate_std.actual_duration_secs) * 100.0;
    let syncopate_tokio_cpu_pct =
        (syncopate_tokio.cpu_time.as_secs_f64() / syncopate_tokio.actual_duration_secs) * 100.0;

    table.add_row(vec![
        Cell::new("CPU Usage"),
        Cell::new(format!("{:.2}", raw_std_cpu_pct)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.2}", raw_tokio_cpu_pct)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.2}", syncopate_std_cpu_pct)).set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.2}", syncopate_tokio_cpu_pct)).set_alignment(CellAlignment::Right),
        Cell::new("%"),
    ]);

    // Memory Usage
    table.add_row(vec![
        Cell::new("Memory Usage"),
        Cell::new(raw_std.memory_kb.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(raw_tokio.memory_kb.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(syncopate_std.memory_kb.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(syncopate_tokio.memory_kb.to_string()).set_alignment(CellAlignment::Right),
        Cell::new("KB"),
    ]);

    // Wakeup Count
    table.add_row(vec![
        Cell::new("Wakeup Count"),
        Cell::new(raw_std.wakeup_count.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(raw_tokio.wakeup_count.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(syncopate_std.wakeup_count.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(syncopate_tokio.wakeup_count.to_string()).set_alignment(CellAlignment::Right),
        Cell::new(""),
    ]);

    println!("\n{}", table);

    // Print summary
    let mut summary_table = Table::new();
    summary_table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Comparison Summary"])
        .add_row(vec![
            "Raw std vs Raw tokio: Measures tokio::time::sleep overhead",
        ])
        .add_row(vec![
            "Raw vs Syncopate: Measures syncopate scheduler overhead",
        ])
        .add_row(vec![
            "Syn+Std vs Syn+Tokio: Measures impact of sleep mechanism on syncopate",
        ]);

    println!("\n{}", summary_table);
}

fn _print_comparison(native: &BenchmarkResults, syncopate: &BenchmarkResults) {
    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "Comparison: Native vs Syncopate",
            "Native",
            "Syncopate",
            "Diff",
        ]);

    table.add_row(vec![
        "Total Executions",
        &native.total_executions.to_string(),
        &syncopate.total_executions.to_string(),
        &format!(
            "{:>+.1}%",
            (syncopate.total_executions as f64 / native.total_executions as f64 - 1.0) * 100.0
        ),
    ]);

    table.add_row(vec![
        "Avg Drift",
        &format!("{:.1}μs", native.avg_drift),
        &format!("{:.1}μs", syncopate.avg_drift),
        &format!(
            "{:>+.1}%",
            if native.avg_drift != 0.0 {
                (syncopate.avg_drift / native.avg_drift - 1.0) * 100.0
            } else {
                0.0
            }
        ),
    ]);

    table.add_row(vec![
        "Max Drift",
        &format!("{:.1}μs", native.max_drift),
        &format!("{:.1}μs", syncopate.max_drift),
        &format!(
            "{:>+.1}%",
            if native.max_drift != 0.0 {
                (syncopate.max_drift / native.max_drift - 1.0) * 100.0
            } else {
                0.0
            }
        ),
    ]);

    table.add_row(vec![
        "Missed",
        &native.missed_count.to_string(),
        &syncopate.missed_count.to_string(),
        &format!(
            "{:>+}",
            syncopate.missed_count as i64 - native.missed_count as i64
        ),
    ]);

    println!("\n{}", table);
}
