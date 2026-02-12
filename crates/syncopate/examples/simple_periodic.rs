use clap::Parser;
use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

const ITERATIONS: usize = 5;

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

fn print_table_header() {
    // Columns: Tick(6), Time Since Last(19), Tasks Executed(35), Sleep Until Next(20)
    println!(
        "┌──────┬───────────────────┬───────────────────────────────────┬────────────────────┐"
    );
    println!(
        "│ Tick │ Time Since Last   │ Tasks Executed                    │ Sleep Until Next   │"
    );
    println!(
        "╞══════╪═══════════════════╪═══════════════════════════════════╪════════════════════╡"
    );
}

fn print_table_row(
    tick_num: usize,
    tick_duration: Duration,
    tasks_str: &str,
    sleep_str: &str,
    is_last: bool,
) {
    println!(
        "│ {:<4} │ {:<17} │ {:<33} │ {:<18} │",
        tick_num,
        format!("{:?}", tick_duration),
        tasks_str,
        sleep_str
    );

    if !is_last {
        println!(
            "├──────┼───────────────────┼───────────────────────────────────┼────────────────────┤"
        );
    }
}

fn print_table_footer() {
    println!(
        "└──────┴───────────────────┴───────────────────────────────────┴────────────────────┘"
    );
}

fn run_mode(mode: Mode) {
    // Create scheduler based on mode
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler = match mode {
        Mode::Real => Scheduler::new(),
        Mode::Emulated => Scheduler::with_test_time(epoch, epoch),
    };

    // Build and add tasks (same for both modes!)
    let tasks = vec![
        TaskBuilder::<()>::every(Duration::from_secs(1))
            .name("every_1s")
            .build()
            .unwrap(),
        TaskBuilder::<()>::every(Duration::from_secs(2))
            .name("every_2s")
            .build()
            .unwrap(),
    ];

    for task in tasks {
        scheduler.add_task(task).unwrap();
    }

    println!("Tasks added: every_1s (1s period), every_2s (2s period)");
    match mode {
        Mode::Real => println!("Running for {ITERATIONS} iterations with real delays...\n"),
        Mode::Emulated => println!("Running for {ITERATIONS} iterations (instant)...\n"),
    }

    print_table_header();

    // Single iteration loop - cleaner with unified API
    let mut elapsed = epoch;
    for tick_num in 1..=5 {
        let tick_duration = Duration::from_secs(1);

        // Branch only for time advancement
        if let Mode::Emulated = mode {
            elapsed += tick_duration;
            scheduler.advance_time(elapsed);
        } else {
            std::thread::sleep(tick_duration);
        }

        // Tick (same for both modes!)
        let fired_tasks = scheduler.tick(tick_duration);

        // Format tasks executed (same for both modes)
        let tasks_str = if fired_tasks.is_empty() {
            "none".to_string()
        } else {
            fired_tasks
                .iter()
                .map(|t| t.name.as_deref().unwrap_or("unnamed"))
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Calculate sleep until next tick (same for both modes)
        let sleep_str = if let Some(next_tick) = scheduler.calculate_next_tick() {
            format!("{:?}", next_tick)
        } else {
            "no tasks".to_string()
        };

        print_table_row(
            tick_num,
            tick_duration,
            &tasks_str,
            &sleep_str,
            tick_num == 5,
        );
    }

    print_table_footer();
}
