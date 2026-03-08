use std::rc::Rc;
use std::time::Duration;

use clap::Parser;
use syncopate::scheduler::Scheduler;
use syncopate::system_time::SimClock;
use syncopate::task::{TaskBuilder, Window};
use syncopate::{InitialTick, MissedTickBehavior, PeriodicSchedule};

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
    initial=<mode>      immediate | delay — fire immediately or after first period (default: immediate)
    schedule=<sched>    fixed-rate | fixed-delay (default: fixed-rate)
    miss=<behavior>     skip | execute | burst | burst:<max> (default: skip)

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

    /// Wall-clock start offset (e.g. 3s means the scheduler starts at 00:03)
    #[arg(long, value_name = "DUR")]
    start: Option<String>,
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
}

struct ScenarioTask {
    name: String,
    period: Duration,
    window: Duration,
    delay: Duration,
    initial_tick: InitialTick,
    schedule: PeriodicSchedule,
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
        // Use the latest end-of-interest: delay + 10 * period per task.
        let longest = self
            .tasks
            .iter()
            .map(|t| t.delay + t.period * 10)
            .max()
            .unwrap_or(Duration::from_millis(5000));
        longest
    }
}

// ── Task/disruption parsing ──────────────────────────────────────────────────

fn parse_task_spec(spec: &str) -> Result<ScenarioTask, String> {
    let mut name = None;
    let mut period = None;
    let mut window = Duration::ZERO;
    let mut delay = Duration::ZERO;
    let mut initial_tick = InitialTick::default();
    let mut schedule = PeriodicSchedule::FixedRate;
    let mut on_miss = MissedTickBehavior::Skip;

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
            "initial" => {
                initial_tick = match value {
                    "immediate" => InitialTick::Immediate,
                    "delay" => InitialTick::Delay,
                    _ => {
                        return Err(format!(
                            "unknown initial mode '{value}' (expected immediate or delay)"
                        ));
                    }
                };
            }
            "schedule" => {
                schedule = match value {
                    "fixed-rate" => PeriodicSchedule::FixedRate,
                    "fixed-delay" => PeriodicSchedule::FixedDelay,
                    _ => {
                        return Err(format!(
                            "unknown schedule '{value}' (expected fixed-rate or fixed-delay)"
                        ));
                    }
                };
            }
            "miss" => {
                on_miss = parse_miss_behavior(value)?;
            }
            _ => return Err(format!("unknown task key '{key}'")),
        }
    }

    let name = name.ok_or("task missing required 'name' key")?;
    let period = period.ok_or("task missing required 'period' key")?;

    Ok(ScenarioTask {
        name,
        period,
        window,
        delay,
        initial_tick,
        schedule,
        on_miss,
    })
}

fn parse_miss_behavior(s: &str) -> Result<MissedTickBehavior, String> {
    match s {
        "skip" => Ok(MissedTickBehavior::Skip),
        "execute" => Ok(MissedTickBehavior::Execute),
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
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Skip,
                },
                ScenarioTask {
                    name: "slow".into(),
                    period: Duration::from_millis(500),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Skip,
                },
            ],
            disruptions: vec![Disruption {
                at: Duration::from_millis(450),
                duration: Duration::from_millis(350),
            }],
            duration_override: None,
            wall_clock_offset: Duration::ZERO,
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
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Burst { max: None },
                },
                ScenarioTask {
                    name: "slow".into(),
                    period: Duration::from_millis(500),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Burst { max: None },
                },
            ],
            disruptions: vec![Disruption {
                at: Duration::from_millis(450),
                duration: Duration::from_millis(350),
            }],
            duration_override: None,
            wall_clock_offset: Duration::ZERO,
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
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Skip,
                },
                ScenarioTask {
                    name: "b".into(),
                    period: Duration::from_millis(300),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Skip,
                },
                ScenarioTask {
                    name: "c".into(),
                    period: Duration::from_millis(500),
                    window: Duration::from_millis(50),
                    delay: Duration::ZERO,
                    initial_tick: InitialTick::default(),
                    schedule: PeriodicSchedule::FixedRate,
                    on_miss: MissedTickBehavior::Execute,
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

fn simulate(scenario: &Scenario) -> Vec<SimEvent> {
    let clock = Rc::new(SimClock::new());
    let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));

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

    // Simulation step: 1/1000th of the smallest period, clamped to [1ns, 1ms].
    let min_period_nanos = scenario
        .tasks
        .iter()
        .map(|t| t.period.as_nanos() as u64)
        .min()
        .unwrap_or(1_000_000);
    let step_nanos = (min_period_nanos / 1000).clamp(1, 1_000_000);

    let mut events = Vec::new();
    let mut current_nanos: u64 = 0;

    // Tick at t=0 to handle Immediate tasks that fire right away.
    record_tick(&scheduler.tick(), current_nanos, &mut events);

    let mut disruptions: Vec<&Disruption> = scenario.disruptions.iter().collect();
    disruptions.sort_by_key(|d| d.at.as_nanos());

    while current_nanos < duration_nanos {
        // Compute this step's target time.
        let step_target = if let Some(d) = disruptions
            .iter()
            .find(|d| d.at.as_nanos() as u64 == current_nanos)
        {
            current_nanos + d.duration.as_nanos() as u64
        } else {
            current_nanos + step_nanos
        };

        // Before advancing, add any pending tasks whose delay falls at or
        // before step_target. Advance the clock to exactly each delay point
        // so the scheduler records the correct anchor time.
        while let Some(&(delay, _)) = pending_tasks.first() {
            if delay <= step_target {
                if delay > current_nanos {
                    clock.advance(Duration::from_nanos(delay - current_nanos));
                    current_nanos = delay;
                }
                let (_, st) = pending_tasks.remove(0);
                add_scenario_task(&mut scheduler, st);
                // Tick immediately so Immediate tasks fire at their delay point.
                let result = scheduler.tick();
                record_tick(&result, current_nanos, &mut events);
            } else {
                break;
            }
        }

        // Advance the clock to the step target.
        if step_target > current_nanos {
            clock.advance(Duration::from_nanos(step_target - current_nanos));
            current_nanos = step_target;
        }

        let result = scheduler.tick();
        record_tick(&result, current_nanos, &mut events);
    }

    events
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
        events.push(SimEvent {
            at_nanos,
            task_name: name.to_string(),
            kind: EventKind::Missed,
        });
    }
}

fn add_scenario_task<C: syncopate::system_time::Clock>(
    scheduler: &mut Scheduler<(), C>,
    st: &ScenarioTask,
) {
    let task: syncopate::task::Task = TaskBuilder::every(st.period, Window::symmetric(st.window))
        .name(&st.name)
        .initial_tick(st.initial_tick)
        .schedule(st.schedule)
        .on_miss(st.on_miss)
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn task_label(task: &ScenarioTask) -> String {
    if task.delay.is_zero() {
        format!("{} ({})", task.name, fmt_duration(task.period))
    } else {
        format!(
            "{} ({}, +{})",
            task.name,
            fmt_duration(task.period),
            fmt_duration(task.delay)
        )
    }
}

const TIMELINE_WIDTH: usize = 120;

fn render(scenario: &Scenario, events: &[SimEvent]) {
    let duration = scenario.duration();
    let duration_nanos = duration.as_nanos() as u64;
    let res_nanos = ((duration_nanos as f64) / (TIMELINE_WIDTH as f64)).ceil() as u64;
    let res_nanos = res_nanos.max(1);
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
    println!(
        "Duration: {}, Resolution: {}/col",
        fmt_duration(duration),
        fmt_duration(Duration::from_nanos(res_nanos))
    );
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

        let period_nanos = task.period.as_nanos() as u64;
        let delay_nanos = task.delay.as_nanos() as u64;
        for col in 0..=cols {
            let col_start = col as u64 * res_nanos;
            let col_end = col_start + res_nanos;
            if has_ideal_deadline(
                period_nanos,
                delay_nanos,
                task.initial_tick,
                col_start,
                col_end,
            ) {
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
            let any = scenario.tasks.iter().any(|t| {
                has_ideal_deadline(
                    t.period.as_nanos() as u64,
                    t.delay.as_nanos() as u64,
                    t.initial_tick,
                    col_start,
                    col_end,
                )
            });
            if any {
                print!("\x1b[36m\u{2193}\x1b[0m");
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

/// Format a wall-clock duration as MM:SS (or HH:MM:SS if >= 1 hour).
fn fmt_wall_clock(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
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

/// Check if there's an ideal deadline in [col_start, col_end).
/// Deadlines are at: offset + period, offset + 2*period, ...
/// (offset is when the task is added; the first fire is one period later.)
fn has_ideal_deadline(
    period_nanos: u64,
    offset_nanos: u64,
    initial_tick: InitialTick,
    col_start: u64,
    col_end: u64,
) -> bool {
    if period_nanos == 0 {
        return false;
    }
    let first_deadline = match initial_tick {
        InitialTick::Immediate => offset_nanos,
        InitialTick::Delay => offset_nanos + period_nanos,
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

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    let scenario = if args.tasks.is_empty() {
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
            s.wall_clock_offset = parse_duration(start_str).unwrap_or_else(|e| die(&e));
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
                .map(|s| parse_duration(s).unwrap_or_else(|e| die(&e)))
                .unwrap_or(Duration::ZERO),
        }
    };

    if scenario.tasks.is_empty() {
        eprintln!("Error: no tasks defined");
        std::process::exit(1);
    }

    let events = simulate(&scenario);
    render(&scenario, &events);
}

fn die(msg: &str) -> ! {
    eprintln!("Error: {msg}");
    std::process::exit(1);
}
