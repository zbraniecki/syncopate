use std::rc::Rc;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use syncopate::{MissedTickBehavior, PeriodicSchedule, Scheduler, SimClock, TaskBuilder, Window};

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(about = "ASCII timeline visualization of scheduler behavior")]
#[command(after_help = "\
TASK FORMAT:
  Comma-separated key=value pairs. Keys:
    name=<str>          Task name (required)
    period=<dur>        Firing period (required)
    window=<dur>        Symmetric window tolerance (default: 0ns)
    delay=<dur>         Initial delay before adding task to scheduler (default: 0ns)
    type=<type>         relative | absolute (default: relative)
    initial=<mode>      immediate | delay — fire immediately or after first period (default: immediate, relative only)
    schedule=<sched>    fixed-rate | fixed-delay (default: fixed-rate, relative only)
    offset=<dur>        Wall-clock offset for absolute tasks (absolute only)
    miss=<behavior>     skip | run-latest | burst | burst:<max> (default: skip)

DISRUPTION FORMAT:
  Comma-separated key=value pairs. Keys:
    at=<dur>            When the disruption starts (required)
    duration=<dur>      How long it lasts (required)

DURATION SYNTAX:
  <number><unit>  where unit is: ns, us, ms, s
  Examples: 100ns, 10us, 250ms, 1s

EXAMPLES:
  # Preset scenario
  visualize --scenario basic

  # Custom tasks with a disruption
  visualize --task name=fast,period=200ms,window=50ms,miss=skip \\
            --task name=slow,period=500ms,window=50ms,miss=burst \\
            --disruption at=450ms,duration=350ms

  # Task added 2s after scheduler start
  visualize --task name=a,period=200ms \\
            --task name=b,period=500ms,delay=2s

  # Anchored task: fire every 1s on the second boundary
  visualize --task name=tick,period=1s,type=anchored,window=50ms

  # Anchored with offset (fire at 500ms past each second)
  visualize --task name=tick,period=1s,type=anchored,offset=500ms,window=50ms

  # Mix of relative and anchored
  visualize --task name=rel,period=200ms,window=50ms \\
            --task name=anch,period=1s,type=anchored,window=50ms

  # Resolution cap: 100ms task with 500ms min tick interval
  visualize --task name=fast,period=100ms,window=10ms \\
            --min-tick-interval 500ms

  # Microsecond-scale tasks
  visualize --task name=hi,period=100us,window=10us \\
            --task name=lo,period=500us \\
            --disruption at=250us,duration=150us")]
struct Args {
    /// Preset scenario: basic, burst, multi (ignored if --task is provided)
    #[arg(long, default_value = "basic")]
    scenario: String,

    /// Task definition (repeatable). Format: name=<n>,period=<dur>[,window=<dur>][,schedule=fixed-rate|fixed-delay][,miss=skip|execute|burst|burst:<max>]
    #[arg(long = "task", value_name = "SPEC")]
    tasks: Vec<String>,

    /// Disruption definition (repeatable). Format: at=<dur>,duration=<dur>
    #[arg(long = "disruption", value_name = "SPEC")]
    disruptions: Vec<String>,

    /// Override scenario duration (auto-computed as 10x longest period if omitted)
    #[arg(long, value_name = "DUR")]
    duration: Option<String>,

    /// Wall-clock start time in HH:mm:ss or HH:mm:ss.zzz format (e.g. 14:30:00, 09:15:30.500)
    #[arg(long, value_name = "TIME")]
    start: Option<String>,

    /// Minimum tick interval (resolution cap). Format: duration (e.g. 500ms)
    #[arg(long, value_name = "DUR")]
    min_tick_interval: Option<String>,

    /// Execution mode: sim (deterministic simulation) or real (actual tokio sleep)
    #[arg(long, default_value = "sim")]
    mode: Mode,

    /// Write scenario fixture (inputs + expected outputs) to a JSON file
    #[arg(long, value_name = "PATH")]
    write_scenario: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    /// Deterministic simulation with SimClock
    Sim,
    /// Real execution with tokio::time::sleep
    Real,
}

// ── Duration parsing ─────────────────────────────────────────────────────────

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num_part, unit) = if let Some(n) = s.strip_suffix("ns") {
        (n, "ns")
    } else if let Some(n) = s.strip_suffix("us") {
        (n, "us")
    } else if let Some(n) = s.strip_suffix("ms") {
        (n, "ms")
    } else if let Some(n) = s.strip_suffix('s') {
        (n, "s")
    } else {
        return Err(format!("missing unit in '{s}' (expected ns, us, ms, or s)"));
    };

    let value: f64 = num_part
        .parse()
        .map_err(|_| format!("invalid number in '{s}'"))?;

    let nanos = match unit {
        "ns" => value,
        "us" => value * 1_000.0,
        "ms" => value * 1_000_000.0,
        "s" => value * 1_000_000_000.0,
        _ => unreachable!(),
    };

    if nanos < 0.0 {
        return Err(format!("duration cannot be negative: '{s}'"));
    }

    Ok(Duration::from_nanos(nanos as u64))
}

/// Parse a wall-clock time string (HH:mm:ss or HH:mm:ss.zzz) into a Duration from midnight.
fn parse_wall_clock(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (time_part, millis) = if let Some((t, frac)) = s.split_once('.') {
        let ms: u64 = frac
            .parse()
            .map_err(|_| format!("invalid milliseconds in '{s}'"))?;
        (t, ms)
    } else {
        (s, 0)
    };

    let parts: Vec<&str> = time_part.split(':').collect();
    if parts.len() != 3 {
        return Err(format!(
            "expected HH:mm:ss or HH:mm:ss.zzz format, got '{s}'"
        ));
    }

    let h: u64 = parts[0]
        .parse()
        .map_err(|_| format!("invalid hours in '{s}'"))?;
    let m: u64 = parts[1]
        .parse()
        .map_err(|_| format!("invalid minutes in '{s}'"))?;
    let sec: u64 = parts[2]
        .parse()
        .map_err(|_| format!("invalid seconds in '{s}'"))?;

    if h >= 24 {
        return Err(format!("hours out of range (0-23) in '{s}'"));
    }
    if m >= 60 {
        return Err(format!("minutes out of range (0-59) in '{s}'"));
    }
    if sec >= 60 {
        return Err(format!("seconds out of range (0-59) in '{s}'"));
    }
    if millis >= 1000 {
        return Err(format!("milliseconds out of range (0-999) in '{s}'"));
    }

    Ok(Duration::from_secs(h * 3600 + m * 60 + sec) + Duration::from_millis(millis))
}

/// Format a duration using the most natural unit for display.
fn fmt_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos == 0 {
        "0".to_string()
    } else if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format_frac(nanos, 1_000, "us")
    } else if nanos < 1_000_000_000 {
        format_frac(nanos, 1_000_000, "ms")
    } else {
        format_frac(nanos, 1_000_000_000, "s")
    }
}

fn format_frac(nanos: u128, divisor: u128, unit: &str) -> String {
    let whole = nanos / divisor;
    let remainder = nanos % divisor;
    if remainder == 0 {
        format!("{}{}", whole, unit)
    } else {
        // Show up to 3 significant fractional digits, trim trailing zeros.
        let frac = (remainder as f64 / divisor as f64 * 1000.0).round() as u64;
        let frac_str = format!("{:03}", frac);
        let frac_str = frac_str.trim_end_matches('0');
        format!("{}.{}{}", whole, frac_str, unit)
    }
}

// ── Scenario definition ──────────────────────────────────────────────────────

struct Scenario {
    name: String,
    description: String,
    tasks: Vec<ScenarioTask>,
    disruptions: Vec<Disruption>,
    duration_override: Option<Duration>,
    wall_clock_offset: Duration,
    min_tick_interval: Option<Duration>,
}

enum ScenarioTaskKind {
    Relative {
        initial_delay: Duration,
        schedule: PeriodicSchedule,
    },
    Absolute {
        offset: Option<Duration>,
    },
}

struct ScenarioTask {
    name: String,
    period: Duration,
    window: Duration,
    delay: Duration,
    kind: ScenarioTaskKind,
    on_miss: MissedTickBehavior,
}

struct Disruption {
    at: Duration,
    duration: Duration,
}

impl Scenario {
    fn duration(&self) -> Duration {
        if let Some(d) = self.duration_override {
            return d;
        }

        let longest_period = self
            .tasks
            .iter()
            .map(|t| t.period.as_nanos() as u64)
            .max()
            .unwrap_or(500_000_000); // 500ms default

        // Compute LCM of all task periods.
        let combined_period = self
            .tasks
            .iter()
            .map(|t| t.period.as_nanos() as u64)
            .fold(1u64, |acc, p| lcm(acc, p));

        let cap = longest_period.saturating_mul(10);

        // 2× LCM so the pattern visibly repeats, capped at 10× longest.
        let base = combined_period.saturating_mul(2).min(cap);

        // Ensure delayed tasks are visible (delay + 2× their period).
        let delay_adjusted = self
            .tasks
            .iter()
            .map(|t| t.delay.as_nanos() as u64 + t.period.as_nanos() as u64 * 2)
            .max()
            .unwrap_or(0);

        // Ensure disruptions fit.
        let disruption_end = self
            .disruptions
            .iter()
            .map(|d| (d.at + d.duration).as_nanos() as u64)
            .max()
            .unwrap_or(0);

        let duration_ns = base.max(delay_adjusted).max(disruption_end);
        Duration::from_nanos(duration_ns)
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    a / gcd(a, b) * b // divide first to reduce overflow risk
}

// ── Task/disruption parsing ──────────────────────────────────────────────────

fn parse_task_spec(spec: &str) -> Result<ScenarioTask, String> {
    let mut name = None;
    let mut period = None;
    let mut window = Duration::ZERO;
    let mut delay = Duration::ZERO;
    let mut initial_delay = None;
    let mut schedule = None;
    let mut on_miss = MissedTickBehavior::Skip;
    let mut task_type: Option<String> = None;
    let mut offset = None;

    for part in spec.split(',') {
        let part = part.trim();
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got '{part}'"))?;

        match key {
            "name" => name = Some(value.to_string()),
            "period" => period = Some(parse_duration(value)?),
            "window" => window = parse_duration(value)?,
            "delay" => delay = parse_duration(value)?,
            "type" => task_type = Some(value.to_string()),
            "offset" => offset = Some(parse_duration(value)?),
            "initial" => {
                initial_delay = Some(match value {
                    "immediate" => Duration::ZERO,
                    "delay" => {
                        // Will be resolved to period below once we know it
                        Duration::MAX
                    }
                    _ => {
                        return Err(format!(
                            "unknown initial mode '{value}' (expected immediate or delay)"
                        ));
                    }
                });
            }
            "schedule" => {
                schedule = Some(match value {
                    "fixed-rate" => PeriodicSchedule::FixedRate,
                    "fixed-delay" => PeriodicSchedule::FixedDelay,
                    _ => {
                        return Err(format!(
                            "unknown schedule '{value}' (expected fixed-rate or fixed-delay)"
                        ));
                    }
                });
            }
            "miss" => {
                on_miss = parse_miss_behavior(value)?;
            }
            _ => return Err(format!("unknown task key '{key}'")),
        }
    }

    let name = name.ok_or("task missing required 'name' key")?;
    let period = period.ok_or("task missing required 'period' key")?;

    // Resolve "delay" sentinel to actual period
    let initial_delay = match initial_delay {
        Some(d) if d == Duration::MAX => period,
        Some(d) => d,
        None => Duration::ZERO,
    };

    let is_absolute =
        task_type.as_deref() == Some("anchored") || task_type.as_deref() == Some("absolute");

    if is_absolute {
        if initial_delay != Duration::ZERO {
            return Err("'initial' is not supported for absolute tasks".into());
        }
        if schedule.is_some() {
            return Err("'schedule' is not supported for absolute tasks".into());
        }
    } else if task_type.is_none() || task_type.as_deref() == Some("relative") {
        if offset.is_some() {
            return Err("'offset' is only supported for absolute tasks".into());
        }
    } else {
        return Err(format!(
            "unknown type '{}' (expected relative or absolute)",
            task_type.unwrap()
        ));
    }

    let kind = if is_absolute {
        ScenarioTaskKind::Absolute { offset }
    } else {
        ScenarioTaskKind::Relative {
            initial_delay,
            schedule: schedule.unwrap_or(PeriodicSchedule::FixedRate),
        }
    };

    Ok(ScenarioTask {
        name,
        period,
        window,
        delay,
        kind,
        on_miss,
    })
}

fn parse_miss_behavior(s: &str) -> Result<MissedTickBehavior, String> {
    match s {
        "skip" => Ok(MissedTickBehavior::Skip),
        "execute" | "run-latest" => Ok(MissedTickBehavior::RunLatest),
        "burst" => Ok(MissedTickBehavior::Burst { max: None }),
        _ if s.starts_with("burst:") => {
            let max: u32 = s[6..]
                .parse()
                .map_err(|_| format!("invalid burst max in '{s}'"))?;
            Ok(MissedTickBehavior::Burst { max: Some(max) })
        }
        _ => Err(format!(
            "unknown miss behavior '{s}' (expected skip, execute, burst, or burst:<max>)"
        )),
    }
}

fn parse_disruption_spec(spec: &str) -> Result<Disruption, String> {
    let mut at = None;
    let mut duration = None;

    for part in spec.split(',') {
        let part = part.trim();
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got '{part}'"))?;

        match key {
            "at" => at = Some(parse_duration(value)?),
            "duration" => duration = Some(parse_duration(value)?),
            _ => return Err(format!("unknown disruption key '{key}'")),
        }
    }

    Ok(Disruption {
        at: at.ok_or("disruption missing required 'at' key")?,
        duration: duration.ok_or("disruption missing required 'duration' key")?,
    })
}

// ── Preset scenarios ─────────────────────────────────────────────────────────

fn get_preset(name: &str) -> Scenario {
    match name {
        "basic" => Scenario {
            name: "basic".into(),
            description: "Two tasks (200ms, 500ms) with a 350ms delay at t=450ms (Skip mode)"
                .into(),
            tasks: vec![
                ScenarioTask {
                    name: "fast".into(),
                    period: Duration::from_millis(200),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::Skip,
                },
                ScenarioTask {
                    name: "slow".into(),
                    period: Duration::from_millis(500),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::Skip,
                },
            ],
            disruptions: vec![Disruption {
                at: Duration::from_millis(450),
                duration: Duration::from_millis(350),
            }],
            duration_override: None,
            wall_clock_offset: Duration::ZERO,
            min_tick_interval: None,
        },
        "burst" => Scenario {
            name: "burst".into(),
            description: "Same tasks with Burst mode — missed deadlines fire in rapid succession"
                .into(),
            tasks: vec![
                ScenarioTask {
                    name: "fast".into(),
                    period: Duration::from_millis(200),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::Burst { max: None },
                },
                ScenarioTask {
                    name: "slow".into(),
                    period: Duration::from_millis(500),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::Burst { max: None },
                },
            ],
            disruptions: vec![Disruption {
                at: Duration::from_millis(450),
                duration: Duration::from_millis(350),
            }],
            duration_override: None,
            wall_clock_offset: Duration::ZERO,
            min_tick_interval: None,
        },
        "multi" => Scenario {
            name: "multi".into(),
            description: "Three tasks (150ms, 300ms, 500ms) with two disruptions".into(),
            tasks: vec![
                ScenarioTask {
                    name: "a".into(),
                    period: Duration::from_millis(150),
                    window: Duration::from_millis(30),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::Skip,
                },
                ScenarioTask {
                    name: "b".into(),
                    period: Duration::from_millis(300),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::Skip,
                },
                ScenarioTask {
                    name: "c".into(),
                    period: Duration::from_millis(500),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    kind: ScenarioTaskKind::Relative {
                        initial_delay: Duration::ZERO,
                        schedule: PeriodicSchedule::FixedRate,
                    },
                    on_miss: MissedTickBehavior::RunLatest,
                },
            ],
            disruptions: vec![
                Disruption {
                    at: Duration::from_millis(400),
                    duration: Duration::from_millis(250),
                },
                Disruption {
                    at: Duration::from_millis(2000),
                    duration: Duration::from_millis(400),
                },
            ],
            duration_override: None,
            wall_clock_offset: Duration::ZERO,
            min_tick_interval: None,
        },
        other => {
            eprintln!("Unknown scenario '{other}'. Available: basic, burst, multi");
            std::process::exit(1);
        }
    }
}

// ── Simulation ───────────────────────────────────────────────────────────────

struct SimEvent {
    at_nanos: u64,
    task_name: String,
    kind: EventKind,
}

enum EventKind {
    Fired,
    Missed,
}

struct SimResult {
    events: Vec<SimEvent>,
    tick_times: Vec<u64>,
}

fn simulate(scenario: &Scenario) -> SimResult {
    let clock = Rc::new(SimClock::new());
    // Set the wall clock to the scenario's wall_clock_offset so anchored tasks
    // fire at the correct wall-clock boundaries.
    if !scenario.wall_clock_offset.is_zero() {
        clock.jump_wall_clock(scenario.wall_clock_offset.as_nanos() as i64);
    }
    let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    if let Some(interval) = scenario.min_tick_interval {
        scheduler.set_min_tick_interval(Some(interval));
    }

    // Track which tasks have been added (deferred by their delay).
    let mut pending_tasks: Vec<(u64, &ScenarioTask)> = scenario
        .tasks
        .iter()
        .map(|st| (st.delay.as_nanos() as u64, st))
        .collect();
    pending_tasks.sort_by_key(|(delay, _)| *delay);

    // Add tasks with zero delay immediately.
    while pending_tasks.first().is_some_and(|(d, _)| *d == 0) {
        let (_, st) = pending_tasks.remove(0);
        add_scenario_task(&mut scheduler, st);
    }

    let duration = scenario.duration();
    let duration_nanos = duration.as_nanos() as u64;

    let mut events = Vec::new();
    let mut tick_times = Vec::new();
    let mut current_nanos: u64 = 0;

    // Tick at t=0 to handle Immediate tasks that fire right away.
    tick_times.push(current_nanos);
    record_tick(&scheduler.tick(), current_nanos, &mut events);

    let mut disruptions: Vec<&Disruption> = scenario.disruptions.iter().collect();
    disruptions.sort_by_key(|d| d.at.as_nanos());

    let mut next_disruption = 0usize;

    while current_nanos < duration_nanos {
        // Check if we've reached a disruption point (jump past its duration).
        if next_disruption < disruptions.len() {
            let d = disruptions[next_disruption];
            let d_at_nanos = d.at.as_nanos() as u64;
            if current_nanos >= d_at_nanos {
                next_disruption += 1;
                let jump = d.duration.as_nanos() as u64;
                clock.advance(Duration::from_nanos(jump));
                current_nanos += jump;
                // Tick after the disruption so the scheduler sees the time jump.
                tick_times.push(current_nanos);
                record_tick(&scheduler.tick(), current_nanos, &mut events);
                continue;
            }
        }

        // Add any pending tasks whose delay has been reached (no tick).
        while let Some(&(delay, _)) = pending_tasks.first() {
            if delay <= current_nanos {
                let (_, st) = pending_tasks.remove(0);
                add_scenario_task(&mut scheduler, st);
            } else {
                break;
            }
        }

        // Use calculate_next_tick() to determine when to advance.
        let sleep_dur = match scheduler.calculate_next_tick() {
            Some(d) => d,
            None => {
                // No tasks scheduled yet. If there are pending tasks, advance
                // to the earliest one's delay, add it (no tick), then re-enter
                // the loop so calculate_next_tick() sees the new task.
                if let Some(&(delay, _)) = pending_tasks.first() {
                    let step = delay.saturating_sub(current_nanos).max(1);
                    clock.advance(Duration::from_nanos(step));
                    current_nanos += step;
                    continue;
                }
                break;
            }
        };

        // Cap to remaining duration.
        let remaining = duration_nanos - current_nanos;
        let advance = (sleep_dur.as_nanos() as u64).min(remaining);
        if advance == 0 {
            // Already at a tick point but calculate_next_tick returned 0;
            // advance by at least 1ns to avoid infinite loop.
            clock.advance(Duration::from_nanos(1));
            current_nanos += 1;
        } else {
            let mut target = current_nanos + advance;

            // Check if a disruption needs to fire before the advance target.
            if next_disruption < disruptions.len() {
                let d_at = disruptions[next_disruption].at.as_nanos() as u64;
                if d_at < target && d_at > current_nanos {
                    target = d_at;
                }
            }

            // Advance toward target, adding pending tasks along the way (no tick).
            while let Some(&(delay, _)) = pending_tasks.first() {
                if delay > current_nanos && delay <= target {
                    // Advance to pending task's delay and add it.
                    let step = delay - current_nanos;
                    clock.advance(Duration::from_nanos(step));
                    current_nanos = delay;
                    let (_, st) = pending_tasks.remove(0);
                    add_scenario_task(&mut scheduler, st);
                } else {
                    break;
                }
            }

            // Advance remaining distance to target.
            let step = target - current_nanos;
            if step > 0 {
                clock.advance(Duration::from_nanos(step));
                current_nanos = target;
            }
        }

        tick_times.push(current_nanos);
        let result = scheduler.tick();
        record_tick(&result, current_nanos, &mut events);
    }

    SimResult { events, tick_times }
}

async fn simulate_real(scenario: &Scenario) -> SimResult {
    let mut scheduler = Scheduler::new();
    if let Some(interval) = scenario.min_tick_interval {
        scheduler.set_min_tick_interval(Some(interval));
    }

    // Track which tasks have been added (deferred by their delay).
    let mut pending_tasks: Vec<(u64, &ScenarioTask)> = scenario
        .tasks
        .iter()
        .map(|st| (st.delay.as_nanos() as u64, st))
        .collect();
    pending_tasks.sort_by_key(|(delay, _)| *delay);

    // Add tasks with zero delay immediately.
    while pending_tasks.first().is_some_and(|(d, _)| *d == 0) {
        let (_, st) = pending_tasks.remove(0);
        add_scenario_task(&mut scheduler, st);
    }

    let duration = scenario.duration();
    let duration_nanos = duration.as_nanos() as u64;

    let mut events = Vec::new();
    let mut tick_times = Vec::new();
    let start = tokio::time::Instant::now();

    // Tick at t=0 to handle Immediate tasks that fire right away.
    tick_times.push(0);
    record_tick(&scheduler.tick(), 0, &mut events);

    let mut disruptions: Vec<&Disruption> = scenario.disruptions.iter().collect();
    disruptions.sort_by_key(|d| d.at.as_nanos());

    // Track which disruptions have already been applied.
    let mut next_disruption = 0usize;

    loop {
        let elapsed_nanos = start.elapsed().as_nanos() as u64;
        if elapsed_nanos >= duration_nanos {
            break;
        }

        // Check if we've reached a disruption point (block for its duration).
        if next_disruption < disruptions.len() {
            let d = disruptions[next_disruption];
            let d_at_nanos = d.at.as_nanos() as u64;
            if elapsed_nanos >= d_at_nanos {
                next_disruption += 1;
                let block_dur = d.duration;
                tokio::time::sleep(block_dur).await;
                // After the disruption, tick to let the scheduler see the time jump.
                let at = start.elapsed().as_nanos() as u64;
                tick_times.push(at);
                record_tick(&scheduler.tick(), at, &mut events);
                continue;
            }
        }

        // Add any pending tasks whose delay has been reached (no tick).
        while let Some(&(delay, _)) = pending_tasks.first() {
            if delay <= elapsed_nanos {
                let (_, st) = pending_tasks.remove(0);
                add_scenario_task(&mut scheduler, st);
            } else {
                break;
            }
        }

        // Sleep until the next tick deadline.
        if let Some(sleep_dur) = scheduler.calculate_next_tick() {
            // Cap sleep so we don't overshoot the scenario duration.
            let remaining = Duration::from_nanos(duration_nanos - elapsed_nanos);
            let capped = sleep_dur.min(remaining);
            tokio::time::sleep(capped).await;
        } else {
            // No tasks scheduled; sleep a small amount and retry.
            tokio::time::sleep(Duration::from_millis(1)).await;
            continue;
        }

        let at = start.elapsed().as_nanos() as u64;
        tick_times.push(at);
        record_tick(&scheduler.tick(), at, &mut events);
    }

    SimResult { events, tick_times }
}

fn record_tick(result: &syncopate::TickResult<'_>, at_nanos: u64, events: &mut Vec<SimEvent>) {
    for exec in &result.fired {
        let name = exec.task.name.as_deref().unwrap_or("?");
        events.push(SimEvent {
            at_nanos,
            task_name: name.to_string(),
            kind: EventKind::Fired,
        });
    }
    for miss in &result.missed {
        let name = miss.task.name.as_deref().unwrap_or("?");
        // Place each missed event at its actual deadline time (tick time - lateness).
        for lateness in &miss.deadlines_missed {
            let deadline_nanos = at_nanos.saturating_sub(lateness.as_nanos() as u64);
            events.push(SimEvent {
                at_nanos: deadline_nanos,
                task_name: name.to_string(),
                kind: EventKind::Missed,
            });
        }
    }
}

fn add_scenario_task<C: syncopate::Clock>(scheduler: &mut Scheduler<(), C>, st: &ScenarioTask) {
    let window = Window::symmetric(st.window);
    let builder = match &st.kind {
        ScenarioTaskKind::Relative {
            initial_delay,
            schedule,
        } => TaskBuilder::every(st.period)
            .window(window)
            .initial_delay(*initial_delay)
            .schedule(*schedule),
        ScenarioTaskKind::Absolute { offset: None } => {
            TaskBuilder::every_absolute(st.period).window(window)
        }
        ScenarioTaskKind::Absolute {
            offset: Some(offset),
        } => TaskBuilder::every_absolute(st.period)
            .offset(*offset)
            .window(window),
    };
    let task = builder.name(&st.name).on_miss(st.on_miss).build().unwrap();
    scheduler.add_task(task).unwrap();
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn task_label(task: &ScenarioTask) -> String {
    let kind_suffix = match &task.kind {
        ScenarioTaskKind::Relative { .. } => String::new(),
        ScenarioTaskKind::Absolute { offset: None } => " [absolute]".into(),
        ScenarioTaskKind::Absolute {
            offset: Some(offset),
        } => format!(" [absolute+{}]", fmt_duration(*offset)),
    };
    if task.delay.is_zero() {
        format!(
            "{} ({}){}",
            task.name,
            fmt_duration(task.period),
            kind_suffix
        )
    } else {
        format!(
            "{} ({}, +{}){}",
            task.name,
            fmt_duration(task.period),
            fmt_duration(task.delay),
            kind_suffix,
        )
    }
}

fn render(scenario: &Scenario, result: &SimResult) {
    let events = &result.events;
    let tick_times = &result.tick_times;
    let duration = scenario.duration();
    let duration_nanos = duration.as_nanos() as u64;
    let ruler_interval = pick_ruler_interval(duration_nanos);
    // Snap resolution so each ruler interval spans exactly 20 columns.
    let res_nanos = (ruler_interval / 20).max(1);
    let cols = (duration_nanos / res_nanos) as usize;

    // Find widest label for padding.
    let label_width = scenario
        .tasks
        .iter()
        .map(|t| task_label(t).len())
        .max()
        .unwrap_or(0)
        .max(
            scenario
                .tasks
                .iter()
                .map(|t| format!("{} actual", t.name).len())
                .max()
                .unwrap_or(0),
        )
        .max("Sched ticks".len())
        .max("Disruptions".len())
        + 2;

    println!();
    println!("Scenario: {} — {}", scenario.name, scenario.description);
    if let Some(min_tick) = scenario.min_tick_interval {
        println!(
            "Duration: {}, Resolution: {}/col, Min tick interval: {}",
            fmt_duration(duration),
            fmt_duration(Duration::from_nanos(res_nanos)),
            fmt_duration(min_tick),
        );
    } else {
        println!(
            "Duration: {}, Resolution: {}/col",
            fmt_duration(duration),
            fmt_duration(Duration::from_nanos(res_nanos))
        );
    }
    println!();

    // Time ruler.
    print_ruler(label_width, cols, res_nanos, duration_nanos);

    // Wall-clock ruler.
    print_clock_ruler(
        label_width,
        cols,
        res_nanos,
        duration_nanos,
        scenario.wall_clock_offset,
    );

    // Per-task ideal rows.
    for task in &scenario.tasks {
        let label = task_label(task);
        print!("{:>width$}  ", label, width = label_width);

        for col in 0..=cols {
            let col_start = col as u64 * res_nanos;
            let col_end = col_start + res_nanos;
            if has_ideal_deadline_for_task(task, scenario.wall_clock_offset, col_start, col_end) {
                print!("\x1b[36m\u{2193}\x1b[0m");
            } else {
                print!("\u{00b7}");
            }
        }
        println!();
    }

    // Scheduler ticks row.
    {
        print!("{:>width$}  ", "Sched ticks", width = label_width);
        for col in 0..=cols {
            let col_start = col as u64 * res_nanos;
            let col_end = col_start + res_nanos;
            let has_ideal = scenario.tasks.iter().any(|t| {
                has_ideal_deadline_for_task(t, scenario.wall_clock_offset, col_start, col_end)
            });
            let has_actual_tick = tick_times.iter().any(|&t| t >= col_start && t < col_end);
            if has_ideal && has_actual_tick {
                // Ideal deadline with an actual tick — normal cyan
                print!("\x1b[36m\u{2193}\x1b[0m");
            } else if has_ideal {
                // Ideal deadline but no tick (skipped due to min_tick_interval) — gray
                print!("\x1b[90m\u{2193}\x1b[0m");
            } else if has_actual_tick {
                // Actual tick at a non-ideal time — cyan dot
                print!("\x1b[36m\u{00b7}\x1b[0m");
            } else {
                print!("\u{00b7}");
            }
        }
        println!();
    }

    // Disruptions row.
    if !scenario.disruptions.is_empty() {
        print!("{:>width$}  ", "Disruptions", width = label_width);
        let mut row = vec![' '; cols + 1];
        for d in &scenario.disruptions {
            let start_col = (d.at.as_nanos() as u64 / res_nanos) as usize;
            let end_col =
                ((d.at.as_nanos() as u64 + d.duration.as_nanos() as u64) / res_nanos) as usize;
            for c in start_col..end_col.min(cols + 1) {
                row[c] = '~';
            }
        }
        for ch in &row {
            if *ch == '~' {
                print!("\x1b[31m~\x1b[0m");
            } else {
                print!(" ");
            }
        }
        println!();
    }

    // Actual execution rows.
    for task in &scenario.tasks {
        let label = format!("{} actual", task.name);
        print!("{:>width$}  ", label, width = label_width);

        let mut row = vec![' '; cols + 1];

        for d in &scenario.disruptions {
            let start_col = (d.at.as_nanos() as u64 / res_nanos) as usize;
            let end_col =
                ((d.at.as_nanos() as u64 + d.duration.as_nanos() as u64) / res_nanos) as usize;
            for c in start_col..end_col.min(cols + 1) {
                row[c] = '~';
            }
        }

        for event in events.iter().filter(|e| e.task_name == task.name) {
            let col = (event.at_nanos / res_nanos) as usize;
            if col <= cols {
                match event.kind {
                    EventKind::Fired => row[col] = 'V',
                    EventKind::Missed => {
                        if row[col] != 'V' {
                            row[col] = 'X';
                        }
                    }
                }
            }
        }

        for ch in &row {
            match ch {
                'V' => print!("\x1b[32m\u{2713}\x1b[0m"),
                'X' => print!("\x1b[31m\u{2717}\x1b[0m"),
                '~' => print!("\x1b[31m~\x1b[0m"),
                _ => print!(" "),
            }
        }
        println!();
    }

    println!();
    println!("Legend:");
    println!(
        "  \x1b[36m\u{2193}\x1b[0m  ideal deadline    \
         \x1b[90m\u{2193}\x1b[0m  skipped (resolution cap)    \
         \x1b[32m\u{2713}\x1b[0m  task fired    \
         \x1b[31m\u{2717}\x1b[0m  missed deadline    \
         \x1b[31m~\x1b[0m  disruption (delay)    \
         \u{00b7}  idle"
    );
    println!();
}

fn print_ruler(label_width: usize, cols: usize, res_nanos: u64, duration_nanos: u64) {
    let ruler_interval = pick_ruler_interval(duration_nanos);

    // The final tick label may extend beyond `cols`; compute the needed width.
    let final_label = fmt_duration(Duration::from_nanos(duration_nanos));
    let buf_len = cols + 1 + final_label.len();
    let mut num_line = vec![b' '; buf_len];
    let mut tick_line = vec![b' '; buf_len];

    let mut tick = 0u64;
    while tick <= duration_nanos {
        let col = (tick / res_nanos) as usize;
        if col <= cols {
            tick_line[col] = b'|';
            let label = fmt_duration(Duration::from_nanos(tick));
            let label_bytes = label.as_bytes();
            if col + label_bytes.len() <= buf_len {
                num_line[col..col + label_bytes.len()].copy_from_slice(label_bytes);
            }
        }
        tick += ruler_interval;
    }

    print!("{:>width$}  ", "Time", width = label_width);
    println!("{}", String::from_utf8_lossy(&num_line));

    print!("{:>width$}  ", "", width = label_width);
    println!("{}", String::from_utf8_lossy(&tick_line));
}

/// Format a wall-clock duration as HH:MM:SS.
fn fmt_wall_clock(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
}

fn print_clock_ruler(
    label_width: usize,
    cols: usize,
    res_nanos: u64,
    duration_nanos: u64,
    wall_clock_offset: Duration,
) {
    let offset_nanos = wall_clock_offset.as_nanos() as u64;
    let end_nanos = offset_nanos + duration_nanos;

    // Pick a ruler interval based on the wall-clock span, same logic as Time.
    let ruler_interval = pick_ruler_interval(duration_nanos);

    // Find the first wall-clock tick that is a multiple of ruler_interval
    // and falls at or after offset_nanos.
    let first_tick = if offset_nanos % ruler_interval == 0 {
        offset_nanos
    } else {
        (offset_nanos / ruler_interval + 1) * ruler_interval
    };

    let final_label = fmt_wall_clock(Duration::from_nanos(end_nanos));
    let buf_len = cols + 1 + final_label.len();
    let mut num_line = vec![b' '; buf_len];
    let mut tick_line = vec![b' '; buf_len];

    let mut wall_tick = first_tick;
    while wall_tick <= end_nanos {
        // Convert wall-clock nanos to column position (relative to offset).
        let rel_nanos = wall_tick - offset_nanos;
        let col = (rel_nanos / res_nanos) as usize;
        if col <= cols {
            tick_line[col] = b'|';
            let label = fmt_wall_clock(Duration::from_nanos(wall_tick));
            let label_bytes = label.as_bytes();
            if col + label_bytes.len() <= buf_len {
                num_line[col..col + label_bytes.len()].copy_from_slice(label_bytes);
            }
        }
        wall_tick += ruler_interval;
    }

    print!("{:>width$}  ", "Clock", width = label_width);
    println!("{}", String::from_utf8_lossy(&num_line));

    print!("{:>width$}  ", "", width = label_width);
    println!("{}", String::from_utf8_lossy(&tick_line));
}

fn pick_ruler_interval(duration_nanos: u64) -> u64 {
    // Candidates in descending order (nanos).
    let candidates: &[u64] = &[
        5_000_000_000, // 5s
        2_000_000_000, // 2s
        1_000_000_000, // 1s
        500_000_000,   // 500ms
        200_000_000,   // 200ms
        100_000_000,   // 100ms
        50_000_000,    // 50ms
        20_000_000,    // 20ms
        10_000_000,    // 10ms
        5_000_000,     // 5ms
        2_000_000,     // 2ms
        1_000_000,     // 1ms
        500_000,       // 500us
        200_000,       // 200us
        100_000,       // 100us
        50_000,        // 50us
        20_000,        // 20us
        10_000,        // 10us
        5_000,         // 5us
        2_000,         // 2us
        1_000,         // 1us
        500,           // 500ns
        200,           // 200ns
        100,           // 100ns
    ];
    for &c in candidates {
        if duration_nanos / c >= 4 {
            return c;
        }
    }
    (duration_nanos / 4).max(1)
}

/// Check if there's an ideal deadline for a relative task in [col_start, col_end).
fn has_ideal_deadline_relative(
    period_nanos: u64,
    delay_nanos: u64,
    initial_delay_nanos: u64,
    col_start: u64,
    col_end: u64,
) -> bool {
    if period_nanos == 0 {
        return false;
    }
    let first_deadline = if initial_delay_nanos == 0 {
        delay_nanos
    } else {
        delay_nanos + initial_delay_nanos
    };
    if col_end <= first_deadline {
        return false;
    }
    if first_deadline >= col_start && first_deadline < col_end {
        return true;
    }
    if col_start < first_deadline {
        return false;
    }
    let shifted_start = col_start - first_deadline;
    let first_k = shifted_start / period_nanos;
    let deadline = first_deadline + first_k * period_nanos;
    if deadline >= col_start && deadline < col_end {
        return true;
    }
    let deadline = first_deadline + (first_k + 1) * period_nanos;
    deadline >= col_start && deadline < col_end
}

/// Check if there's an ideal deadline for an anchored task in [col_start, col_end).
/// Anchored deadlines are at: offset + k*period for k=0,1,2,...
/// Only deadlines >= delay are shown (task isn't added until delay).
fn has_ideal_deadline_anchored(
    period_nanos: u64,
    offset_nanos: u64,
    delay_nanos: u64,
    wall_clock_offset_nanos: u64,
    col_start: u64,
    col_end: u64,
) -> bool {
    if period_nanos == 0 {
        return false;
    }
    // Anchored deadlines fire at wall-clock times: offset + k*period.
    // In relative time (timeline coordinates), that's: offset + k*period - wall_clock_offset.
    // Shift col range into wall-clock space to find matching k.
    let wc_start = col_start + wall_clock_offset_nanos;
    let wc_end = col_end + wall_clock_offset_nanos;

    // Find the first k such that offset + k*period falls in [wc_start, wc_end).
    let first_k = if wc_start <= offset_nanos {
        0
    } else {
        (wc_start - offset_nanos + period_nanos - 1) / period_nanos
    };
    let wc_deadline = offset_nanos + first_k * period_nanos;
    if wc_deadline >= wc_end {
        return false;
    }
    // Convert back to relative time and check delay constraint.
    let rel_deadline = wc_deadline - wall_clock_offset_nanos;
    rel_deadline >= delay_nanos
}

/// Dispatcher: check if a task has an ideal deadline in [col_start, col_end).
fn has_ideal_deadline_for_task(
    task: &ScenarioTask,
    wall_clock_offset: Duration,
    col_start: u64,
    col_end: u64,
) -> bool {
    let period_nanos = task.period.as_nanos() as u64;
    let delay_nanos = task.delay.as_nanos() as u64;
    match &task.kind {
        ScenarioTaskKind::Relative {
            initial_delay,
            schedule: _,
        } => has_ideal_deadline_relative(
            period_nanos,
            delay_nanos,
            initial_delay.as_nanos() as u64,
            col_start,
            col_end,
        ),
        ScenarioTaskKind::Absolute { offset } => {
            let offset_nanos = offset.map_or(0, |o| o.as_nanos() as u64);
            has_ideal_deadline_anchored(
                period_nanos,
                offset_nanos,
                delay_nanos,
                wall_clock_offset.as_nanos() as u64,
                col_start,
                col_end,
            )
        }
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut scenario = if args.tasks.is_empty() {
        // Use preset scenario, optionally with CLI disruption overrides.
        let mut s = get_preset(&args.scenario);
        if !args.disruptions.is_empty() {
            s.disruptions = args
                .disruptions
                .iter()
                .map(|spec| parse_disruption_spec(spec).unwrap_or_else(|e| die(&e)))
                .collect();
        }
        if let Some(ref dur_str) = args.duration {
            s.duration_override = Some(parse_duration(dur_str).unwrap_or_else(|e| die(&e)));
        }
        if let Some(ref start_str) = args.start {
            s.wall_clock_offset = parse_wall_clock(start_str).unwrap_or_else(|e| die(&e));
        }
        s
    } else {
        // Fully custom scenario from CLI.
        let tasks: Vec<ScenarioTask> = args
            .tasks
            .iter()
            .map(|spec| parse_task_spec(spec).unwrap_or_else(|e| die(&e)))
            .collect();
        let disruptions: Vec<Disruption> = args
            .disruptions
            .iter()
            .map(|spec| parse_disruption_spec(spec).unwrap_or_else(|e| die(&e)))
            .collect();

        let description = format!(
            "{} task(s), {} disruption(s)",
            tasks.len(),
            disruptions.len()
        );

        Scenario {
            name: "custom".into(),
            description,
            tasks,
            disruptions,
            duration_override: args
                .duration
                .as_ref()
                .map(|s| parse_duration(s).unwrap_or_else(|e| die(&e))),
            wall_clock_offset: args
                .start
                .as_ref()
                .map(|s| parse_wall_clock(s).unwrap_or_else(|e| die(&e)))
                .unwrap_or(Duration::ZERO),
            min_tick_interval: None,
        }
    };

    if let Some(ref interval_str) = args.min_tick_interval {
        scenario.min_tick_interval = Some(parse_duration(interval_str).unwrap_or_else(|e| die(&e)));
    }

    if scenario.tasks.is_empty() {
        eprintln!("Error: no tasks defined");
        std::process::exit(1);
    }

    // In real mode, default wall_clock_offset to the current time-of-day
    // (unless explicitly set via --start).
    if matches!(args.mode, Mode::Real) && args.start.is_none() {
        let now = jiff::Zoned::now();
        let h = now.hour() as u64;
        let m = now.minute() as u64;
        let s = now.second() as u64;
        let ns = now.subsec_nanosecond() as u64;
        scenario.wall_clock_offset =
            Duration::from_secs(h * 3600 + m * 60 + s) + Duration::from_nanos(ns);
    }

    let result = match args.mode {
        Mode::Sim => simulate(&scenario),
        Mode::Real => simulate_real(&scenario).await,
    };
    render(&scenario, &result);

    if let Some(ref path) = args.write_scenario {
        write_fixture(&scenario, &result, path);
    }
}

fn write_fixture(scenario: &Scenario, result: &SimResult, path: &str) {
    #[cfg(not(feature = "serde"))]
    {
        let _ = (scenario, result, path);
        eprintln!(
            "Error: --write-scenario requires the 'serde' feature. \
             Re-run with: cargo run --example visualize --features serde -- ..."
        );
        std::process::exit(1);
    }

    #[cfg(feature = "serde")]
    {
        use syncopate::fixture::*;

        let input = ScenarioInput {
            tasks: scenario
                .tasks
                .iter()
                .map(|t| {
                    let (window_early_ns, window_late_ns) =
                        (t.window.as_nanos() as u64, t.window.as_nanos() as u64);
                    let kind = match &t.kind {
                        ScenarioTaskKind::Relative {
                            initial_delay,
                            schedule,
                        } => TaskKindDef::Relative {
                            initial_delay_ns: initial_delay.as_nanos() as u64,
                            schedule: *schedule,
                        },
                        ScenarioTaskKind::Absolute { offset } => TaskKindDef::Absolute {
                            offset_ns: offset.map_or(0, |o| o.as_nanos() as u64),
                        },
                    };
                    TaskDef {
                        name: t.name.clone(),
                        period_ns: t.period.as_nanos() as u64,
                        window_early_ns,
                        window_late_ns,
                        delay_ns: t.delay.as_nanos() as u64,
                        kind,
                        on_miss: t.on_miss,
                        repeat: None,
                    }
                })
                .collect(),
            disruptions: scenario
                .disruptions
                .iter()
                .map(|d| DisruptionDef {
                    at_ns: d.at.as_nanos() as u64,
                    duration_ns: d.duration.as_nanos() as u64,
                })
                .collect(),
            duration_ns: scenario.duration_override.map(|d| d.as_nanos() as u64),
            wall_clock_offset_ns: scenario.wall_clock_offset.as_nanos() as u64,
            min_tick_interval_ns: scenario.min_tick_interval.map(|d| d.as_nanos() as u64),
        };

        let expected = ScenarioOutput {
            events: result
                .events
                .iter()
                .map(|e| EventDef {
                    at_ns: e.at_nanos,
                    task_name: e.task_name.clone(),
                    kind: match e.kind {
                        EventKind::Fired => EventKindDef::Fired,
                        EventKind::Missed => EventKindDef::Missed,
                    },
                })
                .collect(),
            tick_times_ns: result.tick_times.clone(),
        };

        let fixture = Fixture {
            name: scenario.name.clone(),
            description: scenario.description.clone(),
            input,
            expected,
        };

        let json = serde_json::to_string_pretty(&fixture).expect("failed to serialize fixture");
        std::fs::write(path, &json).unwrap_or_else(|e| {
            eprintln!("Error writing fixture to '{path}': {e}");
            std::process::exit(1);
        });
        eprintln!("Fixture written to {path}");
    }
}

fn die(msg: &str) -> ! {
    eprintln!("Error: {msg}");
    std::process::exit(1);
}
