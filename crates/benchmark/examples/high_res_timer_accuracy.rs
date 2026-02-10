use clap::Parser;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static EXACT_COUNT: AtomicU64 = AtomicU64::new(0);
static EARLY_COUNT: AtomicU64 = AtomicU64::new(0);
static LATE_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Parser, Debug)]
#[command(name = "high_res_timer_accuracy")]
#[command(about = "High-resolution timer accuracy test using hybrid sleep + spinlock")]
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

    /// Microseconds to spinlock before target time (0 to disable)
    #[arg(long, default_value = "500")]
    spinlock_us: u64,
}

fn fps_to_interval_micros(fps: u32) -> u64 {
    (1_000_000.0 / fps as f64) as u64
}

fn main() {
    let args = Args::parse();

    let interval_micros = fps_to_interval_micros(args.fps);
    let interval = Duration::from_micros(interval_micros);
    let tolerance = Duration::from_millis(args.tolerance);
    let test_duration = Duration::from_secs(args.duration);
    let spinlock_threshold = Duration::from_micros(args.spinlock_us);

    println!("High-Resolution Timer Accuracy Test");
    println!("====================================");
    println!(
        "Target FPS: {} ({:.2} ms per frame)",
        args.fps,
        interval_micros as f64 / 1000.0
    );
    println!("Tolerance: ±{} ms", args.tolerance);
    println!("Duration: {} seconds", args.duration);
    println!("Spinlock: {} µs before target", args.spinlock_us);
    println!();

    let start = Instant::now();
    let mut last_tick: Option<Instant> = None;
    let mut next_tick = start + interval;

    loop {
        let now = Instant::now();

        // High-precision timing: sleep until close to target, then spin-wait
        if now < next_tick {
            let remaining = next_tick - now;

            if remaining > spinlock_threshold + Duration::from_micros(100) {
                // Sleep for most of the duration, leaving buffer for spinlock
                std::thread::sleep(remaining - spinlock_threshold - Duration::from_micros(50));
            }

            // Spin-wait for the remaining time (high-precision)
            while Instant::now() < next_tick {
                std::hint::spin_loop();
            }
        }

        // Now execute the tick
        let tick_time = Instant::now();

        if let Some(last) = last_tick {
            let actual_interval = tick_time - last;
            let diff_micros = actual_interval.as_micros() as i64 - interval_micros as i64;
            let tolerance_micros = tolerance.as_micros() as i64;

            if diff_micros < -tolerance_micros {
                EARLY_COUNT.fetch_add(1, Ordering::SeqCst);
            } else if diff_micros > tolerance_micros {
                LATE_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                EXACT_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        last_tick = Some(tick_time);
        next_tick = tick_time + interval;

        if start.elapsed() >= test_duration {
            break;
        }
    }

    let exact = EXACT_COUNT.load(Ordering::SeqCst);
    let early = EARLY_COUNT.load(Ordering::SeqCst);
    let late = LATE_COUNT.load(Ordering::SeqCst);
    let total = exact + early + late;

    let interval_ms = interval_micros as f64 / 1000.0;
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
    let expected_ticks = (args.duration * 1_000_000) as f64 / interval_micros as f64;
    println!(
        "Expected ~{:.0} frames in {} seconds at {} FPS",
        expected_ticks, args.duration, args.fps
    );
}
