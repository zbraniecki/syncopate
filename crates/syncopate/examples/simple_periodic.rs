use std::rc::Rc;
use std::time::{Duration, Instant};

use clap::Parser;
use jiff::Zoned;
use syncopate::scheduler::Scheduler;
use syncopate::system_time::{Clock, SimClock};
use syncopate::task::{TaskBuilder, Window};

const ITERATIONS: usize = 3;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Mode: 'real' uses real clock with actual delays, 'emulated' runs instantly.
    #[arg(long, default_value = "emulated")]
    mode: String,
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Real,
    Emulated,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Real => write!(f, "Real (uses actual time and sleeps)"),
            Mode::Emulated => write!(f, "Emulated (instant, no delays)"),
        }
    }
}

fn main() {
    let args = Args::parse();

    let mode = match args.mode.as_str() {
        "real" => Mode::Real,
        "emulated" => Mode::Emulated,
        other => {
            eprintln!("Invalid mode '{other}'. Use 'real' or 'emulated'");
            std::process::exit(1);
        }
    };

    println!("=== Simple Periodic Task Example ===");
    println!("Mode: {}\n", mode);

    // Construction is mode-specific because the clock type differs.
    // Everything after that is shared via the generic run_loop.
    match mode {
        Mode::Real => {
            let mut scheduler = Scheduler::new();
            scheduler.set_timer_delay(Duration::from_millis(5));
            add_tasks(&mut scheduler);
            run_loop(mode, &mut scheduler, |requested| {
                // Sleep for the requested duration; return how long we actually slept.
                let t = Instant::now();
                std::thread::sleep(requested);
                t.elapsed()
            });
        }
        Mode::Emulated => {
            let clock = Rc::new(SimClock::new());
            let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
            add_tasks(&mut scheduler);
            run_loop(mode, &mut scheduler, |requested| {
                // Advance the sim clock by exactly the requested amount; no jitter.
                clock.advance(requested);
                requested
            });
        }
    }

    println!("\n=== Example Complete ===");
}

// ── Shared setup ──────────────────────────────────────────────────────────────

fn add_tasks<C: Clock>(scheduler: &mut Scheduler<(), C>) {
    let task = TaskBuilder::every(
        Duration::from_millis(500),
        Window::symmetric(Duration::from_millis(100)),
    )
    .name("every_500ms")
    .build()
    .unwrap();

    scheduler.add_task(task).unwrap();
    println!("Task added: every_500ms (period: 500ms, window: ±100ms)");
}

// ── Generic loop ──────────────────────────────────────────────────────────────

/// One row's worth of data, produced by the scheduler loop and consumed by the
/// printer. Keeping this struct as the boundary means run_loop has no knowledge
/// of how output is formatted, and print_row has no knowledge of the scheduler.
struct TickRow {
    tick_num: usize,
    time: String,
    tick_duration: Option<Duration>,
    slept_for: Option<Duration>,
    sync_work: Option<Duration>,
    tasks_executed: String,
    tasks_missed: String,
    /// Signed total drift for this tick in nanoseconds (sum across all tasks).
    drift_ns: i128,
}

/// Run the scheduler loop for `ITERATIONS` ticks.
///
/// `wait` is the only mode-specific piece: given the *requested* sleep duration
/// it performs the wait and returns the *actual* elapsed duration.
///
/// - Real mode:     sleeps the OS thread, measures actual elapsed time.
/// - Emulated mode: advances the sim clock by the exact requested amount.
fn run_loop<C: Clock>(
    mode: Mode,
    scheduler: &mut Scheduler<(), C>,
    mut wait: impl FnMut(Duration) -> Duration,
) {
    match mode {
        Mode::Real => println!("Running for {ITERATIONS} iterations with real delays...\n"),
        Mode::Emulated => println!("Running for {ITERATIONS} iterations (instant)...\n"),
    }

    print_table_header();

    let mut cumulative_drift = 0i128;

    // Compute the first sleep before the loop. Each subsequent call happens
    // at the end of the iteration, after ALL sync work (tick + printing),
    // so calculate_next_tick() naturally accounts for processing time.
    let mut tick_duration = scheduler.calculate_next_tick();

    for tick_num in 1..=ITERATIONS {
        let (slept_for, work_start, tick_result) = if let Some(requested) = tick_duration {
            let actual = wait(requested);
            let work_start = Instant::now();
            let result = scheduler.tick();
            (Some(actual), Some(work_start), result)
        } else {
            (
                None,
                None,
                syncopate::TickResult {
                    fired: vec![],
                    missed: vec![],
                },
            )
        };

        // Extract data from tick_result before releasing the borrow on scheduler.
        let tick_drift: i128 = tick_result
            .fired
            .iter()
            .map(|e| e.drift.as_nanos_signed())
            .sum::<i128>()
            + tick_result
                .missed
                .iter()
                .map(|e| e.drift.as_nanos_signed())
                .sum::<i128>();
        cumulative_drift += tick_drift;
        let tasks_executed = format_tasks(tick_result.fired.iter());
        let tasks_missed = format_tasks(tick_result.missed.iter());
        drop(tick_result);

        let row = TickRow {
            tick_num,
            time: Zoned::now().strftime("%H:%M:%S%.3f").to_string(),
            tick_duration,
            slept_for,
            sync_work: work_start.map(|s| s.elapsed()),
            tasks_executed,
            tasks_missed,
            drift_ns: tick_drift,
        };

        print_table_row(&row, tick_num == ITERATIONS);

        // Calculate next tick AFTER all sync work (tick + printing) is done.
        // The scheduler reads `now` here, so the returned sleep naturally
        // compensates for all processing time in this iteration.
        tick_duration = scheduler.calculate_next_tick();
    }

    print_table_footer(cumulative_drift);
}

fn format_tasks<'a, Ctx: 'a>(
    iter: impl Iterator<Item = &'a syncopate::TaskExecution<'a, Ctx>>,
) -> String {
    let v: Vec<String> = iter
        .map(|e| {
            let name = e.task.name.as_deref().unwrap_or("unnamed");
            format!("{} ({})", name, e.drift)
        })
        .collect();
    if v.is_empty() {
        "none".to_string()
    } else {
        v.join(", ")
    }
}

// ── Table rendering ───────────────────────────────────────────────────────────

const COL_WIDTHS: [usize; 8] = [4, 18, 18, 12, 12, 25, 25, 12];

fn print_horizontal_line(left: &str, mid: &str, right: &str, fill: &str) {
    print!("{left}");
    for (i, &w) in COL_WIDTHS.iter().enumerate() {
        print!("{}", fill.repeat(w + 2));
        if i < COL_WIDTHS.len() - 1 {
            print!("{mid}");
        }
    }
    println!("{right}");
}

fn print_table_header() {
    print_horizontal_line("┌", "┬", "┐", "─");
    println!(
        "│ {:<w0$} │ {:<w1$} │ {:<w2$} │ {:<w3$} │ {:<w4$} │ {:<w5$} │ {:<w6$} │ {:<w7$} │",
        "Tick",
        "Scheduled sleep",
        "Woke up after",
        "Time",
        "Sync work",
        "Tasks Executed",
        "Tasks Missed",
        "Total Drift",
        w0 = COL_WIDTHS[0],
        w1 = COL_WIDTHS[1],
        w2 = COL_WIDTHS[2],
        w3 = COL_WIDTHS[3],
        w4 = COL_WIDTHS[4],
        w5 = COL_WIDTHS[5],
        w6 = COL_WIDTHS[6],
        w7 = COL_WIDTHS[7],
    );
    print_horizontal_line("╞", "╪", "╡", "═");
}

fn print_table_row(row: &TickRow, is_last: bool) {
    println!(
        "│ {:<w0$} │ {:<w1$} │ {:<w2$} │ {:<w3$} │ {:<w4$} │ {:<w5$} │ {:<w6$} │ {:<w7$} │",
        row.tick_num,
        fmt_opt_duration(row.tick_duration),
        fmt_opt_duration(row.slept_for),
        row.time,
        fmt_opt_duration(row.sync_work),
        row.tasks_executed,
        row.tasks_missed,
        fmt_drift_ns(row.drift_ns),
        w0 = COL_WIDTHS[0],
        w1 = COL_WIDTHS[1],
        w2 = COL_WIDTHS[2],
        w3 = COL_WIDTHS[3],
        w4 = COL_WIDTHS[4],
        w5 = COL_WIDTHS[5],
        w6 = COL_WIDTHS[6],
        w7 = COL_WIDTHS[7],
    );
    if !is_last {
        print_horizontal_line("├", "┼", "┤", "─");
    }
}

fn print_table_footer(cumulative_drift: i128) {
    print_horizontal_line("└", "┴", "┘", "─");
    println!("Cumulative drift: {}", fmt_drift_ns(cumulative_drift));
}

/// Format a `Duration` with the most readable unit, preserving full precision
/// and trimming trailing zeros — matching what `{:?}` produces but without
/// the `Some(...)` wrapper.
///
/// Examples: `494.9665ms`, `500.008041ms`, `1.5µs`, `42ns`, `2.001s`
fn fmt_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        fmt_frac(nanos, 1_000, 3, "µs")
    } else if nanos < 1_000_000_000 {
        fmt_frac(nanos, 1_000_000, 6, "ms")
    } else {
        fmt_frac(nanos, 1_000_000_000, 9, "s")
    }
}

/// Format `nanos` as `{whole}.{frac}{unit}`, where `frac` has exactly
/// `frac_digits` zero-padded digits with trailing zeros trimmed.
/// If the remainder is zero, the fractional part is omitted entirely.
fn fmt_frac(nanos: u128, divisor: u128, frac_digits: usize, unit: &str) -> String {
    let whole = nanos / divisor;
    let remainder = nanos % divisor;
    if remainder == 0 {
        format!("{}{}", whole, unit)
    } else {
        let frac = format!("{:0>width$}", remainder, width = frac_digits);
        let frac = frac.trim_end_matches('0');
        format!("{}.{}{}", whole, frac, unit)
    }
}

/// Format an `Option<Duration>`, using `—` for `None`.
fn fmt_opt_duration(d: Option<Duration>) -> String {
    match d {
        Some(d) => fmt_duration(d),
        None => "—".to_string(),
    }
}

/// Format a signed nanosecond value with the most readable unit and sign prefix.
fn fmt_drift_ns(ns: i128) -> String {
    if ns == 0 {
        return "0ns".to_string();
    }
    let sign = if ns < 0 { "-" } else { "+" };
    let abs = ns.unsigned_abs();
    if abs < 1_000 {
        format!("{}{}ns", sign, abs)
    } else if abs < 1_000_000 {
        format!("{}{}µs", sign, abs / 1_000)
    } else if abs < 1_000_000_000 {
        format!("{}{}ms", sign, abs / 1_000_000)
    } else {
        let secs = abs / 1_000_000_000;
        let ms = (abs % 1_000_000_000) / 1_000_000;
        if ms == 0 {
            format!("{}{}s", sign, secs)
        } else {
            format!("{}{}.{:03}s", sign, secs, ms)
        }
    }
}
