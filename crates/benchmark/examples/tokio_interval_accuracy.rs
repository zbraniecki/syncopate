use clap::Parser;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::time::interval;

static EXACT_COUNT: AtomicU64 = AtomicU64::new(0);
static EARLY_COUNT: AtomicU64 = AtomicU64::new(0);
static LATE_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Parser, Debug)]
#[command(name = "tokio_interval_accuracy")]
#[command(about = "Measures tokio interval timing accuracy at different FPS")]
struct Args {
    /// Target FPS (24, 30, or 60)
    #[arg(short, long, default_value = "60")]
    fps: u32,

    /// Duration to run the test in seconds
    #[arg(short, long, default_value = "5")]
    duration: u64,

    /// Tolerance in milliseconds for "exact" timing
    #[arg(short, long, default_value = "1")]
    tolerance: u64,
}

fn fps_to_interval_ms(fps: u32) -> f64 {
    1000.0 / fps as f64
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Validate FPS
    let valid_fps = match args.fps {
        24 | 30 | 60 => args.fps,
        _ => {
            eprintln!(
                "Warning: Non-standard FPS {}. Supported: 24, 30, 60",
                args.fps
            );
            args.fps
        }
    };

    let interval_ms = fps_to_interval_ms(valid_fps);
    let interval_duration = Duration::from_secs_f64(interval_ms / 1000.0);
    let tolerance = Duration::from_millis(args.tolerance);
    let test_duration = Duration::from_secs(args.duration);

    println!("Tokio Interval Timing Accuracy Test");
    println!("=====================================");
    println!("Target FPS: {} ({} ms per frame)", valid_fps, interval_ms);
    println!("Tolerance: ±{} ms", args.tolerance);
    println!("Duration: {} seconds", args.duration);
    println!();

    let start = Instant::now();
    let mut ticker = interval(interval_duration);
    let mut last_tick: Option<Instant> = None;

    loop {
        ticker.tick().await;
        let now = Instant::now();

        if let Some(last) = last_tick {
            let actual_interval = now - last;
            let expected_micros = (interval_ms * 1000.0) as i64;
            let actual_micros = actual_interval.as_micros() as i64;
            let diff_micros = actual_micros - expected_micros;
            let tolerance_micros = tolerance.as_micros() as i64;

            if diff_micros < -tolerance_micros {
                EARLY_COUNT.fetch_add(1, Ordering::SeqCst);
            } else if diff_micros > tolerance_micros {
                LATE_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                EXACT_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        last_tick = Some(now);

        if start.elapsed() >= test_duration {
            break;
        }
    }

    let exact = EXACT_COUNT.load(Ordering::SeqCst);
    let early = EARLY_COUNT.load(Ordering::SeqCst);
    let late = LATE_COUNT.load(Ordering::SeqCst);
    let total = exact + early + late;

    let min_exact_ms = interval_ms - args.tolerance as f64;
    let max_exact_ms = interval_ms + args.tolerance as f64;

    println!("Results:");
    println!("  Total intervals measured: {}", total);
    if total > 0 {
        println!(
            "  Exact ({:.2}-{:.2} ms):  {} ({:.1}%)",
            min_exact_ms,
            max_exact_ms,
            exact,
            100.0 * exact as f64 / total as f64
        );
        println!(
            "  Early (<{:.2} ms):       {} ({:.1}%)",
            min_exact_ms,
            early,
            100.0 * early as f64 / total as f64
        );
        println!(
            "  Late (>{:.2} ms):        {} ({:.1}%)",
            max_exact_ms,
            late,
            100.0 * late as f64 / total as f64
        );
    }
    println!();
    let expected_ticks = (args.duration * 1000) as f64 / interval_ms;
    println!(
        "Expected ~{:.0} frames in {} seconds at {} FPS",
        expected_ticks, args.duration, valid_fps
    );
}
