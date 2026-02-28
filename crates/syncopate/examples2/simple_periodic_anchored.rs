use clap::Parser;
use jiff::Zoned;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::{TaskBuilder, Window};

const ITERATIONS: usize = 15;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Mode: 'real' uses real clock with actual delays, 'emulated' runs instantly
    #[arg(long, default_value = "emulated")]
    mode: String,
}

fn main() {
    let args = Args::parse();

    let mode = match args.mode.as_str() {
        "real" => Mode::Real,
        "emulated" => Mode::Emulated,
        _ => {
            eprintln!("Invalid mode '{}'. Use 'real' or 'emulated'", args.mode);
            std::process::exit(1);
        }
    };

    println!("=== Simple Periodic Task Example ===");
    println!("Mode: {}\n", mode);

    run_mode(mode);

    println!("\n=== Example Complete ===");
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

const COL_WIDTHS: [usize; 7] = [4, 18, 18, 12, 25, 25, 12];

fn print_horizontal_line(left: &str, mid: &str, right: &str, fill: &str) {
    print!("{}", left);
    for (i, &width) in COL_WIDTHS.iter().enumerate() {
        print!("{}", fill.repeat(width + 2)); // +2 for padding spaces
        if i < COL_WIDTHS.len() - 1 {
            print!("{}", mid);
        }
    }
    println!("{}", right);
}

fn print_table_header() {
    print_horizontal_line("┌", "┬", "┐", "─");
    println!(
        "│ {:<width0$} │ {:<width1$} │ {:<width2$} │ {:<width3$} │ {:<width4$} │ {:<width5$} │ {:<width6$} │",
        "Tick",
        "Scheduled sleep",
        "Woke up after",
        "Time",
        "Tasks Executed",
        "Tasks Missed",
        "Total Drift",
        width0 = COL_WIDTHS[0],
        width1 = COL_WIDTHS[1],
        width2 = COL_WIDTHS[2],
        width3 = COL_WIDTHS[3],
        width4 = COL_WIDTHS[4],
        width5 = COL_WIDTHS[5],
        width6 = COL_WIDTHS[6],
    );
    print_horizontal_line("╞", "╪", "╡", "═");
}

fn print_table_row(
    tick_num: usize,
    time: String,
    tick_duration: Option<Duration>,
    slept: Option<Duration>,
    tasks_executed: &str,
    tasks_missed: &str,
    total_drift: &str,
    is_last: bool,
) {
    println!(
        "│ {:<width0$} │ {:<width1$} │ {:<width2$} │ {:<width3$} │ {:<width4$} │ {:<width5$} │ {:<width6$} │",
        tick_num,
        format!("{:?}", tick_duration),
        format!("{:?}", slept),
        time,
        tasks_executed,
        tasks_missed,
        total_drift,
        width0 = COL_WIDTHS[0],
        width1 = COL_WIDTHS[1],
        width2 = COL_WIDTHS[2],
        width3 = COL_WIDTHS[3],
        width4 = COL_WIDTHS[4],
        width5 = COL_WIDTHS[5],
        width6 = COL_WIDTHS[6],
    );

    if !is_last {
        print_horizontal_line("├", "┼", "┤", "─");
    }
}

fn print_table_footer(cumulative_drift: i64) {
    print_horizontal_line("└", "┴", "┘", "─");
    println!("Cumulative drift: {}ms", cumulative_drift);
}

fn run_mode(mode: Mode) {
    // Create scheduler based on mode
    let (mut scheduler, epoch): (Scheduler<()>, SystemTime) = match mode {
        Mode::Real => {
            // Align epoch to wall-clock boundaries for absolute timing
            // Round current time down to the nearest second
            let now = SystemTime::now();
            let since_unix = now.duration_since(UNIX_EPOCH).unwrap();
            let seconds_since_unix = since_unix.as_secs();
            let aligned_epoch = UNIX_EPOCH + Duration::from_secs(seconds_since_unix);

            // every_at_boundary(500ms) -> fires at :00.000, :00.500, :01.000, :01.500...
            // every_at_boundary(1s) -> fires at :00.000, :01.000, :02.000...
            (
                Scheduler::with_test_time_and_delay(aligned_epoch, now, Duration::from_millis(5)),
                aligned_epoch,
            )
        }
        Mode::Emulated => {
            let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
            (Scheduler::with_test_time(epoch, epoch), epoch)
        }
    };

    // Build and add tasks (same for both modes!)
    let tasks = vec![
        TaskBuilder::every_at_boundary(
            Duration::from_millis(500),
            Window::symmetric(Duration::from_millis(100)),
        )
        .name("every_500ms")
        .build()
        .unwrap(),
    ];

    for task in tasks {
        scheduler.add_task(task).unwrap();
    }

    println!("Task added: every_500ms (fires at wall-clock half-second boundaries)");
    match mode {
        Mode::Real => println!("Running for {ITERATIONS} iterations with real delays...\n"),
        Mode::Emulated => println!("Running for {ITERATIONS} iterations (instant)...\n"),
    }

    print_table_header();

    // Single iteration loop - cleaner with unified API
    let mut elapsed = epoch;
    let max_ticks = ITERATIONS;
    let mut cumulative_drift = 0i64;

    for tick_num in 1..=max_ticks {
        let tick_duration = scheduler.calculate_next_tick();

        let now = Instant::now();
        let (slept_for, tick_result) = if let Some(tick_duration) = tick_duration {
            let (actual_duration, result) = if let Mode::Emulated = mode {
                // Emulated: advance by scheduled amount
                elapsed += tick_duration;
                scheduler.advance_time(elapsed);
                let result = scheduler.tick(tick_duration);
                (tick_duration, result)
            } else {
                // Real: sleep, measure actual time, then tick
                std::thread::sleep(tick_duration);
                let slept = now.elapsed();
                let result = scheduler.tick(slept);
                (slept, result)
            };
            (Some(actual_duration), result)
        } else {
            (
                None,
                syncopate::TickResult {
                    fired: vec![],
                    missed: vec![],
                },
            )
        };

        // Format tasks executed with drift
        let tasks_executed_str = if tick_result.fired.is_empty() {
            "none".to_string()
        } else {
            tick_result
                .fired
                .iter()
                .map(|exec| {
                    let name = exec.task.name.as_deref().unwrap_or("unnamed");
                    format!("{} ({:+}ms)", name, exec.drift_ms)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Format tasks missed with drift
        let tasks_missed_str = if tick_result.missed.is_empty() {
            "none".to_string()
        } else {
            tick_result
                .missed
                .iter()
                .map(|exec| {
                    let name = exec.task.name.as_deref().unwrap_or("unnamed");
                    format!("{} ({:+}ms)", name, exec.drift_ms)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Calculate total drift for this tick (sum of absolute values)
        let tick_drift: i64 = tick_result
            .fired
            .iter()
            .map(|e| e.drift_ms.abs())
            .sum::<i64>()
            + tick_result
                .missed
                .iter()
                .map(|e| e.drift_ms.abs())
                .sum::<i64>();
        cumulative_drift += tick_drift;

        // Get current time using jiff
        let time_str = Zoned::now().strftime("%H:%M:%S%.3f").to_string();

        print_table_row(
            tick_num,
            time_str,
            tick_duration,
            slept_for,
            &tasks_executed_str,
            &tasks_missed_str,
            &format!("{}ms", tick_drift),
            tick_num == max_ticks,
        );
    }

    print_table_footer(cumulative_drift);
}
