use std::time::{Duration, Instant};

use clap::Parser;
use comfy_table::{Table, presets::UTF8_FULL};
use syncopate::scheduler::Scheduler;
use syncopate::task::{PeriodicSchedule, TaskBuilder, Window};
use tokio::sync::mpsc;

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "bench_compare",
    about = "Compare syncopate against independent tokio-style intervals"
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
}

#[derive(Clone)]
enum Termination {
    Duration(Duration),
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
    fn fires_per_wakeup(&self) -> f64 {
        if self.wakeups == 0 {
            return 0.0;
        }
        self.fires as f64 / self.wakeups as f64
    }

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

// ── TokioIntervalBackend ────────────────────────────────────────────────────
//
// Simulates N independent `tokio::time::interval` timers — one per task,
// no coalescing, no window concept. Each fire resets the clock: the next
// deadline is `now + period`. Drift = how far the actual inter-fire gap
// deviated from the target period. No misses — every deadline fires.

struct TokioIntervalBackend;

struct TokioTaskState {
    period: Duration,
    next_deadline: Duration,
    last_fire: Option<Duration>,
}

impl SchedulerBackend for TokioIntervalBackend {
    fn name(&self) -> &str {
        "emulated"
    }

    fn run(&self, scenario: &Scenario) -> ScenarioResult {
        let mut tasks: Vec<TokioTaskState> = scenario
            .tasks
            .iter()
            .map(|spec| match spec {
                TaskSpec::RelativePeriodic { period, .. } => TokioTaskState {
                    period: *period,
                    next_deadline: *period,
                    last_fire: None,
                },
            })
            .collect();

        let start = Instant::now();
        let mut wakeups = 0usize;
        let mut fires = 0usize;
        let mut drifts_ns = Vec::new();

        let Termination::Duration(run_dur) = &scenario.termination;

        loop {
            let earliest = tasks
                .iter()
                .map(|t| t.next_deadline)
                .min()
                .expect("no tasks");

            let elapsed = start.elapsed();
            if elapsed >= *run_dur {
                break;
            }

            if earliest > elapsed {
                std::thread::sleep(earliest - elapsed);
            }

            let now = start.elapsed();
            wakeups += 1;

            for task in &mut tasks {
                while task.next_deadline <= now {
                    // Drift = how far this fire deviated from the ideal period
                    // since the last fire. First fire measures from t=0.
                    let expected = match task.last_fire {
                        Some(last) => last + task.period,
                        None => task.period,
                    };
                    let drift_ns = now.as_nanos() as i128 - expected.as_nanos() as i128;
                    fires += 1;
                    drifts_ns.push(drift_ns);

                    task.last_fire = Some(now);
                    // Fixed-delay: next deadline is now + period.
                    task.next_deadline = now + task.period;
                }
            }

            if now >= *run_dur {
                break;
            }
        }

        ScenarioResult {
            backend_name: self.name().to_string(),
            wakeups,
            fires,
            misses: 0, // tokio-interval never misses
            drifts_ns,
        }
    }
}

// ── RealTokioBackend ────────────────────────────────────────────────────────
//
// Uses actual `tokio::time::interval` timers to measure real tokio drift.
// Wakeups are approximated via an mpsc channel: one recv() = one wakeup,
// then try_recv() drains any fires that arrived in the same poll cycle.

struct RealTokioBackend;

impl SchedulerBackend for RealTokioBackend {
    fn name(&self) -> &str {
        "tokio-interval"
    }

    fn run(&self, scenario: &Scenario) -> ScenarioResult {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

        rt.block_on(async {
            let Termination::Duration(run_dur) = &scenario.termination;
            let run_dur = *run_dur;

            let (tx, mut rx) = mpsc::unbounded_channel::<(usize, Instant)>();

            let start = Instant::now();

            // Spawn one task per interval
            for (task_id, spec) in scenario.tasks.iter().enumerate() {
                let period = match spec {
                    TaskSpec::RelativePeriodic { period, .. } => *period,
                };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(period);
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    // Skip the immediate first tick
                    interval.tick().await;
                    loop {
                        interval.tick().await;
                        let now = Instant::now();
                        if now.duration_since(start) >= run_dur {
                            break;
                        }
                        if tx.send((task_id, now)).is_err() {
                            break;
                        }
                    }
                });
            }
            // Drop our copy so the channel closes when all senders finish
            drop(tx);

            let num_tasks = scenario.tasks.len();
            let periods: Vec<Duration> = scenario
                .tasks
                .iter()
                .map(|spec| match spec {
                    TaskSpec::RelativePeriodic { period, .. } => *period,
                })
                .collect();

            let mut last_fire: Vec<Option<Instant>> = vec![None; num_tasks];
            let mut wakeups = 0usize;
            let mut fires = 0usize;
            let mut drifts_ns = Vec::new();

            // Collector loop
            while let Some((task_id, fire_time)) = rx.recv().await {
                wakeups += 1;

                // Process this fire
                let expected = match last_fire[task_id] {
                    Some(last) => last + periods[task_id],
                    None => start + periods[task_id],
                };
                let drift_ns = fire_time.duration_since(start).as_nanos() as i128
                    - expected.duration_since(start).as_nanos() as i128;
                drifts_ns.push(drift_ns);
                last_fire[task_id] = Some(fire_time);
                fires += 1;

                // Drain any fires that arrived in the same poll cycle
                while let Ok((task_id, fire_time)) = rx.try_recv() {
                    let expected = match last_fire[task_id] {
                        Some(last) => last + periods[task_id],
                        None => start + periods[task_id],
                    };
                    let drift_ns = fire_time.duration_since(start).as_nanos() as i128
                        - expected.duration_since(start).as_nanos() as i128;
                    drifts_ns.push(drift_ns);
                    last_fire[task_id] = Some(fire_time);
                    fires += 1;
                }
            }

            ScenarioResult {
                backend_name: self.name().to_string(),
                wakeups,
                fires,
                misses: 0,
                drifts_ns,
            }
        })
    }
}

// ── SyncopateBackend ─────────────────────────────────────────────────────────
//
// Measures inter-fire gap drift externally — the same way tokio-interval does:
// capture Instant::now() right after wakeup, drift = gap - period.
//
// Uses FixedRate + sleep_until so the scheduler self-corrects for consistent
// OS oversleep (just like tokio::time::interval does internally). The
// scheduler's own exec.drift is NOT used because it includes overhead between
// sleep return and tick()'s internal clock read.

struct SyncopateBackend {
    timer_delay: Duration,
    label: &'static str,
}

impl SchedulerBackend for SyncopateBackend {
    fn name(&self) -> &str {
        self.label
    }

    fn run(&self, scenario: &Scenario) -> ScenarioResult {
        // Capture epoch right before scheduler creation so it aligns with
        // the RealClock's internal mono_epoch.
        let epoch = Instant::now();
        let mut scheduler: Scheduler<()> = Scheduler::new();
        scheduler.set_timer_delay(self.timer_delay);

        let periods: Vec<Duration> = scenario
            .tasks
            .iter()
            .map(|spec| match spec {
                TaskSpec::RelativePeriodic { period, .. } => *period,
            })
            .collect();

        for spec in &scenario.tasks {
            let task = match spec {
                TaskSpec::RelativePeriodic {
                    name,
                    period,
                    window,
                } => TaskBuilder::every(*period, Window::new(window.early, window.late))
                    .name(name.clone())
                    .schedule(PeriodicSchedule::FixedRate)
                    .build()
                    .unwrap(),
            };
            scheduler.add_task(task).unwrap();
        }

        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async {
            let start = Instant::now();
            let mut wakeups = 0usize;
            let mut fires = 0usize;
            let mut misses = 0usize;
            let mut drifts_ns = Vec::new();
            let mut last_fire: Vec<Option<Instant>> = vec![None; scenario.tasks.len()];

            loop {
                let deadline_mono = match scheduler.next_tick_deadline() {
                    Some(d) => d,
                    None => break,
                };

                let std_instant =
                    epoch + Duration::from_nanos(deadline_mono.as_nanos() as u64);
                tokio::time::sleep_until(tokio::time::Instant::from_std(std_instant))
                    .await;

                // Capture time immediately after wakeup — before tick() overhead.
                let now = Instant::now();

                let result = scheduler.tick();

                wakeups += 1;
                for (i, _exec) in result.fired.iter().enumerate() {
                    fires += 1;
                    // Inter-fire gap drift, same metric as tokio-interval.
                    let task_idx = i % periods.len();
                    let expected = match last_fire[task_idx] {
                        Some(last) => last + periods[task_idx],
                        None => start + periods[task_idx],
                    };
                    let drift_ns = now.duration_since(start).as_nanos() as i128
                        - expected.duration_since(start).as_nanos() as i128;
                    drifts_ns.push(drift_ns);
                    last_fire[task_idx] = Some(now);
                }
                for exec in &result.missed {
                    misses += exec.deadlines_missed.len();
                    for d in &exec.deadlines_missed {
                        drifts_ns.push(d.as_nanos() as i128);
                    }
                }

                let Termination::Duration(dur) = &scenario.termination;
                if start.elapsed() >= *dur {
                    break;
                }
            }

            ScenarioResult {
                backend_name: self.name().to_string(),
                wakeups,
                fires,
                misses,
                drifts_ns,
            }
        })
    }
}

// ── Scenarios ────────────────────────────────────────────────────────────────

fn build_scenarios(scale: f64) -> Vec<Scenario> {
    let scale_termination = |t: Termination| -> Termination {
        let Termination::Duration(d) = t;
        Termination::Duration(Duration::from_secs_f64(d.as_secs_f64() * scale))
    };

    let w5 = WindowSpec {
        early: Duration::from_millis(5),
        late: Duration::from_millis(10),
    };

    let dur3s = |s: &str| -> Scenario {
        Scenario {
            name: s.to_string(),
            tasks: Vec::new(),
            termination: scale_termination(Termination::Duration(Duration::from_secs(3))),
        }
    };

    vec![
        // 1. Single 100ms — baseline, both backends identical
        Scenario {
            tasks: vec![TaskSpec::RelativePeriodic {
                name: "rel_100ms".into(),
                period: Duration::from_millis(100),
                window: w5.clone(),
            }],
            ..dur3s("Single 100ms")
        },
        // 2. Single 16ms (~60fps) — baseline at high frequency
        Scenario {
            tasks: vec![TaskSpec::RelativePeriodic {
                name: "rel_16ms".into(),
                period: Duration::from_millis(16),
                window: w5.clone(),
            }],
            ..dur3s("Single 16ms")
        },
        // 3. 3 near periods
        Scenario {
            tasks: vec![
                TaskSpec::RelativePeriodic {
                    name: "rel_97ms".into(),
                    period: Duration::from_millis(97),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_100ms".into(),
                    period: Duration::from_millis(100),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_103ms".into(),
                    period: Duration::from_millis(103),
                    window: w5.clone(),
                },
            ],
            ..dur3s("3 Near Periods")
        },
        // 4. 5 clustered
        Scenario {
            tasks: vec![
                TaskSpec::RelativePeriodic {
                    name: "rel_95ms".into(),
                    period: Duration::from_millis(95),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_98ms".into(),
                    period: Duration::from_millis(98),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_100ms".into(),
                    period: Duration::from_millis(100),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_102ms".into(),
                    period: Duration::from_millis(102),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_105ms".into(),
                    period: Duration::from_millis(105),
                    window: w5.clone(),
                },
            ],
            ..dur3s("5 Clustered")
        },
        // 5. Non-overlapping — control
        Scenario {
            tasks: vec![
                TaskSpec::RelativePeriodic {
                    name: "rel_50ms".into(),
                    period: Duration::from_millis(50),
                    window: w5.clone(),
                },
                TaskSpec::RelativePeriodic {
                    name: "rel_150ms".into(),
                    period: Duration::from_millis(150),
                    window: w5.clone(),
                },
            ],
            ..dur3s("Non-overlapping")
        },
    ]
}

// ── Formatting helpers ───────────────────────────────────────────────────────

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

fn print_scenario_results(results: &[ScenarioResult]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);

    table.set_header(vec![
        "Backend",
        "Wakeups",
        "Fires",
        "Misses",
        "Fires/wakeup",
        "Drift avg",
        "Drift p95",
        "Drift max",
    ]);

    for r in results {
        table.add_row(vec![
            r.backend_name.clone(),
            r.wakeups.to_string(),
            r.fires.to_string(),
            r.misses.to_string(),
            format!("{:.1}", r.fires_per_wakeup()),
            format_drift_f64(r.drift_avg()),
            format_drift_f64(r.drift_p95()),
            format_drift_f64(r.drift_max()),
        ]);
    }

    println!("{table}");
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    let scenarios = build_scenarios(args.scale);

    let backends: Vec<Box<dyn SchedulerBackend>> = vec![
        Box::new(TokioIntervalBackend),
        Box::new(RealTokioBackend),
        Box::new(SyncopateBackend {
            timer_delay: Duration::ZERO,
            label: "syncopate",
        }),
        Box::new(SyncopateBackend {
            timer_delay: Duration::from_millis(5),
            label: "syncopate+5ms",
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
        println!("\n=== {} ===\n", scenario.name);

        let mut results = Vec::new();
        for backend in &filtered_backends {
            eprint!("  {:15} ...", backend.name());
            let result = backend.run(scenario);
            eprintln!(" {} wakeups, {} fires", result.wakeups, result.fires);
            results.push(result);
        }

        print_scenario_results(&results);
    }
}
