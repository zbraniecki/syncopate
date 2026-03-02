use std::time::{Duration, Instant};

use clap::Parser;
use comfy_table::{Table, presets::UTF8_FULL};
use syncopate::scheduler::Scheduler;
use syncopate::task::{TaskBuilder, Window};

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "bench_compare",
    about = "Compare syncopate against a naive polling scheduler"
)]
struct Args {
    /// Substring filter on scenario names.
    #[arg(long)]
    scenario: Option<String>,

    /// Substring filter on backend names.
    #[arg(long)]
    backend: Option<String>,

    /// Duration multiplier (e.g. 0.1 for quick runs). Only scales scenario
    /// duration, NOT task periods.
    #[arg(long, default_value_t = 1.0)]
    scale: f64,
}

// ── Spec types ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct WindowSpec {
    early: Duration,
    late: Duration,
}

#[derive(Clone)]
enum TaskSpec {
    RelativePeriodic {
        name: String,
        period: Duration,
        window: WindowSpec,
    },
    AnchoredPeriodic {
        name: String,
        period: Duration,
        offset: Option<Duration>,
        window: WindowSpec,
    },
    OneShot {
        name: String,
        delay: Duration,
        window: WindowSpec,
    },
}

#[derive(Clone)]
enum Termination {
    Duration(Duration),
    UntilAllFired,
}

#[derive(Clone)]
struct Scenario {
    name: String,
    tasks: Vec<TaskSpec>,
    termination: Termination,
}

// ── ScenarioResult ───────────────────────────────────────────────────────────

struct ScenarioResult {
    backend_name: String,
    wakeups: usize,
    fires: usize,
    misses: usize,
    drifts_ns: Vec<i128>,
}

impl ScenarioResult {
    fn drift_max(&self) -> f64 {
        self.drifts_ns
            .iter()
            .map(|d| d.unsigned_abs() as f64)
            .fold(0.0_f64, f64::max)
    }

    fn drift_avg(&self) -> f64 {
        if self.drifts_ns.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.drifts_ns.iter().map(|d| d.unsigned_abs() as f64).sum();
        sum / self.drifts_ns.len() as f64
    }

    fn drift_p95(&self) -> f64 {
        if self.drifts_ns.is_empty() {
            return 0.0;
        }
        let mut abs: Vec<f64> = self
            .drifts_ns
            .iter()
            .map(|d| d.unsigned_abs() as f64)
            .collect();
        abs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((abs.len() as f64) * 0.95).ceil() as usize;
        abs[idx.min(abs.len() - 1)]
    }
}

// ── Backend trait ────────────────────────────────────────────────────────────

trait SchedulerBackend {
    fn name(&self) -> &str;
    fn run(&self, scenario: &Scenario) -> ScenarioResult;
}

// ── SyncopateBackend ─────────────────────────────────────────────────────────

struct SyncopateBackend;

impl SchedulerBackend for SyncopateBackend {
    fn name(&self) -> &str {
        "syncopate"
    }

    fn run(&self, scenario: &Scenario) -> ScenarioResult {
        let mut scheduler: Scheduler<()> = Scheduler::new();
        scheduler.set_timer_delay(Duration::from_millis(5));

        for spec in &scenario.tasks {
            let task = match spec {
                TaskSpec::RelativePeriodic {
                    name,
                    period,
                    window,
                } => TaskBuilder::every(*period, Window::new(window.early, window.late))
                    .name(name.clone())
                    .build()
                    .unwrap(),
                TaskSpec::AnchoredPeriodic {
                    name,
                    period,
                    offset,
                    window,
                } => {
                    if let Some(off) = offset {
                        TaskBuilder::every_with_offset(
                            *period,
                            *off,
                            Window::new(window.early, window.late),
                        )
                        .name(name.clone())
                        .build()
                        .unwrap()
                    } else {
                        TaskBuilder::every_at_boundary(
                            *period,
                            Window::new(window.early, window.late),
                        )
                        .name(name.clone())
                        .build()
                        .unwrap()
                    }
                }
                TaskSpec::OneShot {
                    name,
                    delay,
                    window,
                } => TaskBuilder::once_after(*delay, Window::new(window.early, window.late))
                    .name(name.clone())
                    .build()
                    .unwrap(),
            };
            scheduler.add_task(task).unwrap();
        }

        let start = Instant::now();
        let safety_cap = Duration::from_secs(60);
        let mut wakeups = 0usize;
        let mut fires = 0usize;
        let mut misses = 0usize;
        let mut drifts_ns = Vec::new();

        loop {
            let sleep_dur = match scheduler.calculate_next_tick() {
                Some(d) => d,
                None => break, // all tasks exhausted
            };

            std::thread::sleep(sleep_dur);
            let result = scheduler.tick();
            wakeups += 1;

            for exec in &result.fired {
                fires += 1;
                drifts_ns.push(exec.drift.as_nanos_signed());
            }
            for exec in &result.missed {
                misses += 1;
                drifts_ns.push(exec.drift.as_nanos_signed());
            }

            match &scenario.termination {
                Termination::Duration(dur) => {
                    if start.elapsed() >= *dur {
                        break;
                    }
                }
                Termination::UntilAllFired => {
                    if start.elapsed() >= safety_cap {
                        break;
                    }
                }
            }
        }

        ScenarioResult {
            backend_name: self.name().to_string(),
            wakeups,
            fires,
            misses,
            drifts_ns,
        }
    }
}

// ── NaivePollingBackend ──────────────────────────────────────────────────────

struct NaivePollingBackend {
    poll_interval: Duration,
}

struct NaiveTaskState {
    period: Option<Duration>,
    next_deadline: Duration,
    window_late: Duration,
    one_shot: bool,
    fired: bool,
}

impl NaivePollingBackend {
    fn build_task_states(scenario: &Scenario) -> Vec<NaiveTaskState> {
        scenario
            .tasks
            .iter()
            .map(|spec| match spec {
                TaskSpec::RelativePeriodic { period, window, .. } => NaiveTaskState {
                    period: Some(*period),
                    next_deadline: *period,
                    window_late: window.late,
                    one_shot: false,
                    fired: false,
                },
                // The naive backend treats anchored tasks the same as relative
                // (elapsed-since-start). This is intentional — the comparison
                // measures wakeup efficiency, not wall-clock alignment correctness.
                TaskSpec::AnchoredPeriodic { period, window, .. } => NaiveTaskState {
                    period: Some(*period),
                    next_deadline: *period,
                    window_late: window.late,
                    one_shot: false,
                    fired: false,
                },
                TaskSpec::OneShot { delay, window, .. } => NaiveTaskState {
                    period: None,
                    next_deadline: *delay,
                    window_late: window.late,
                    one_shot: true,
                    fired: false,
                },
            })
            .collect()
    }
}

impl SchedulerBackend for NaivePollingBackend {
    fn name(&self) -> &str {
        "naive (10ms)"
    }

    fn run(&self, scenario: &Scenario) -> ScenarioResult {
        let mut task_states = Self::build_task_states(scenario);
        let start = Instant::now();
        let safety_cap = Duration::from_secs(60);
        let mut wakeups = 0usize;
        let mut fires = 0usize;
        let mut misses = 0usize;
        let mut drifts_ns = Vec::new();

        loop {
            std::thread::sleep(self.poll_interval);
            wakeups += 1;
            let elapsed = start.elapsed();

            for task in &mut task_states {
                if task.one_shot && task.fired {
                    continue;
                }

                let deadline = task.next_deadline;
                let window_end = deadline + task.window_late;

                if elapsed >= deadline && elapsed <= window_end {
                    // Within window → fire
                    let drift_ns = elapsed.as_nanos() as i128 - deadline.as_nanos() as i128;
                    fires += 1;
                    drifts_ns.push(drift_ns);
                    if task.one_shot {
                        task.fired = true;
                    } else if let Some(period) = task.period {
                        task.next_deadline = deadline + period;
                    }
                } else if elapsed > window_end {
                    // Past window → miss
                    let drift_ns = elapsed.as_nanos() as i128 - deadline.as_nanos() as i128;
                    misses += 1;
                    drifts_ns.push(drift_ns);
                    if task.one_shot {
                        task.fired = true;
                    } else if let Some(period) = task.period {
                        task.next_deadline = deadline + period;
                    }
                }
            }

            // Check termination
            let all_one_shots_done = task_states.iter().filter(|t| t.one_shot).all(|t| t.fired);
            let has_only_one_shots = task_states.iter().all(|t| t.one_shot);

            match &scenario.termination {
                Termination::Duration(dur) => {
                    if elapsed >= *dur {
                        break;
                    }
                }
                Termination::UntilAllFired => {
                    if has_only_one_shots && all_one_shots_done {
                        break;
                    }
                    if elapsed >= safety_cap {
                        break;
                    }
                }
            }
        }

        ScenarioResult {
            backend_name: self.name().to_string(),
            wakeups,
            fires,
            misses,
            drifts_ns,
        }
    }
}

// ── Scenarios ────────────────────────────────────────────────────────────────

fn build_scenarios(scale: f64) -> Vec<Scenario> {
    let scale_termination = |t: Termination| -> Termination {
        match t {
            Termination::Duration(d) => {
                Termination::Duration(Duration::from_secs_f64(d.as_secs_f64() * scale))
            }
            other => other,
        }
    };

    let w50 = WindowSpec {
        early: Duration::from_millis(50),
        late: Duration::from_millis(50),
    };

    vec![
        Scenario {
            name: "Single Relative (1s period, 30s)".to_string(),
            tasks: vec![TaskSpec::RelativePeriodic {
                name: "rel_1s".to_string(),
                period: Duration::from_secs(1),
                window: w50.clone(),
            }],
            termination: scale_termination(Termination::Duration(Duration::from_secs(30))),
        },
        Scenario {
            name: "Single Anchored (1s period, 30s)".to_string(),
            tasks: vec![TaskSpec::AnchoredPeriodic {
                name: "anch_1s".to_string(),
                period: Duration::from_secs(1),
                offset: None,
                window: w50.clone(),
            }],
            termination: scale_termination(Termination::Duration(Duration::from_secs(30))),
        },
        Scenario {
            name: "One-Shot (5s)".to_string(),
            tasks: vec![TaskSpec::OneShot {
                name: "once_5s".to_string(),
                delay: Duration::from_secs(5),
                window: w50.clone(),
            }],
            termination: Termination::UntilAllFired,
        },
        Scenario {
            name: "Mixed (30s)".to_string(),
            tasks: vec![
                TaskSpec::RelativePeriodic {
                    name: "rel_500ms".to_string(),
                    period: Duration::from_millis(500),
                    window: w50.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_1s".to_string(),
                    period: Duration::from_secs(1),
                    window: w50.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_2s".to_string(),
                    period: Duration::from_secs(2),
                    window: w50.clone(),
                },
            ],
            termination: scale_termination(Termination::Duration(Duration::from_secs(30))),
        },
    ]
}

// ── Formatting helpers ───────────────────────────────────────────────────────

#[allow(dead_code)]
fn format_drift_ns(ns: i128) -> String {
    if ns == 0 {
        return "0ns".to_string();
    }
    let sign = if ns < 0 { "-" } else { "+" };
    let abs = ns.unsigned_abs();
    if abs < 1_000 {
        format!("{}{}ns", sign, abs)
    } else if abs < 1_000_000 {
        format!("{}{:.1}µs", sign, abs as f64 / 1_000.0)
    } else if abs < 1_000_000_000 {
        format!("{}{:.1}ms", sign, abs as f64 / 1_000_000.0)
    } else {
        format!("{}{:.3}s", sign, abs as f64 / 1_000_000_000.0)
    }
}

fn format_drift_f64(ns: f64) -> String {
    if ns == 0.0 {
        return "0ns".to_string();
    }
    let abs = ns.abs();
    if abs < 1_000.0 {
        format!("+{:.0}ns", abs)
    } else if abs < 1_000_000.0 {
        format!("+{:.1}µs", abs / 1_000.0)
    } else if abs < 1_000_000_000.0 {
        format!("+{:.1}ms", abs / 1_000_000.0)
    } else {
        format!("+{:.3}s", abs / 1_000_000_000.0)
    }
}

// ── Output ───────────────────────────────────────────────────────────────────

fn print_scenario_results(scenario: &Scenario, results: &[ScenarioResult]) {
    println!("\n=== Scenario: {} ===\n", scenario.name);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);

    let mut header = vec!["Metric".to_string()];
    for r in results {
        header.push(r.backend_name.clone());
    }
    table.set_header(header);

    // Wakeups
    let mut row = vec!["Wakeups".to_string()];
    for r in results {
        row.push(r.wakeups.to_string());
    }
    table.add_row(row);

    // Fires
    let mut row = vec!["Fires".to_string()];
    for r in results {
        row.push(r.fires.to_string());
    }
    table.add_row(row);

    // Misses
    let mut row = vec!["Misses".to_string()];
    for r in results {
        row.push(r.misses.to_string());
    }
    table.add_row(row);

    // Drift max
    let mut row = vec!["Drift max".to_string()];
    for r in results {
        row.push(format_drift_f64(r.drift_max()));
    }
    table.add_row(row);

    // Drift avg
    let mut row = vec!["Drift avg".to_string()];
    for r in results {
        row.push(format_drift_f64(r.drift_avg()));
    }
    table.add_row(row);

    // Drift p95
    let mut row = vec!["Drift p95".to_string()];
    for r in results {
        row.push(format_drift_f64(r.drift_p95()));
    }
    table.add_row(row);

    println!("{table}");
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    let scenarios = build_scenarios(args.scale);
    let backends: Vec<Box<dyn SchedulerBackend>> = vec![
        Box::new(SyncopateBackend),
        Box::new(NaivePollingBackend {
            poll_interval: Duration::from_millis(10),
        }),
    ];

    let filtered_scenarios: Vec<&Scenario> = scenarios
        .iter()
        .filter(|s| {
            args.scenario
                .as_ref()
                .is_none_or(|f| s.name.to_lowercase().contains(&f.to_lowercase()))
        })
        .collect();

    let filtered_backends: Vec<&Box<dyn SchedulerBackend>> = backends
        .iter()
        .filter(|b| {
            args.backend
                .as_ref()
                .is_none_or(|f| b.name().to_lowercase().contains(&f.to_lowercase()))
        })
        .collect();

    if filtered_scenarios.is_empty() {
        eprintln!("No scenarios matched the filter.");
        std::process::exit(1);
    }
    if filtered_backends.is_empty() {
        eprintln!("No backends matched the filter.");
        std::process::exit(1);
    }

    println!(
        "Running {} scenario(s) x {} backend(s) (scale={:.1})\n",
        filtered_scenarios.len(),
        filtered_backends.len(),
        args.scale,
    );

    for scenario in &filtered_scenarios {
        let mut results = Vec::new();

        for backend in &filtered_backends {
            eprint!(
                "  Running {:30} with {:15}...",
                scenario.name,
                backend.name()
            );
            let result = backend.run(scenario);
            eprintln!(" done ({} wakeups, {} fires)", result.wakeups, result.fires);
            results.push(result);
        }

        print_scenario_results(scenario, &results);
    }
}
