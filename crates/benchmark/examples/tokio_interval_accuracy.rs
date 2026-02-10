use clap::Parser;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct AppCtx {
    exact: AtomicU64,
    late: AtomicU64,
    early: AtomicU64,
    last_tick: Option<Instant>,
    interval_ms: f64,
    tolerance: Duration,
}

impl AppCtx {
    pub fn new(interval_ms: f64, tolerance: Duration) -> Self {
        Self {
            exact: AtomicU64::new(0),
            late: AtomicU64::new(0),
            early: AtomicU64::new(0),
            last_tick: None,
            interval_ms,
            tolerance,
        }
    }
}

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

// #[tokio::main]
fn main() {
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
    let tolerance = Duration::from_millis(args.tolerance);
    let test_duration = Duration::from_secs(args.duration);

    println!("Tokio Interval Timing Accuracy Test");
    println!("=====================================");
    println!("Target FPS: {} ({} ms per frame)", valid_fps, interval_ms);
    println!("Tolerance: ±{} ms", args.tolerance);
    println!("Duration: {} seconds", args.duration);
    println!();

    let mut ctx = AppCtx::new(interval_ms, tolerance);

    let start = Instant::now();

    loop {
        tick(&mut ctx);

        if start.elapsed() >= test_duration {
            break;
        }

        let now = Instant::now();
        std::thread::sleep(Duration::from_secs(1));
        println!("elapsed: {:?}", now.elapsed());
    }

    let exact = ctx.exact.load(Ordering::SeqCst);
    let early = ctx.early.load(Ordering::SeqCst);
    let late = ctx.late.load(Ordering::SeqCst);
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

fn tick(ctx: &mut AppCtx) {
    let now = Instant::now();

    if let Some(last) = ctx.last_tick {
        let actual_interval = now - last;
        let expected_micros = (ctx.interval_ms * 1000.0) as i64;
        let actual_micros = actual_interval.as_micros() as i64;
        let diff_micros = actual_micros - expected_micros;
        let tolerance_micros = ctx.tolerance.as_micros() as i64;

        if diff_micros < -tolerance_micros {
            ctx.early.fetch_add(1, Ordering::SeqCst);
        } else if diff_micros > tolerance_micros {
            ctx.late.fetch_add(1, Ordering::SeqCst);
        } else {
            ctx.exact.fetch_add(1, Ordering::SeqCst);
        }
    }

    ctx.last_tick = Some(now);
}
