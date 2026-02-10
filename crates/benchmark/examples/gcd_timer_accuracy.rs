//! High-resolution timer accuracy test using macOS GCD (Grand Central Dispatch) timers
//!
//! This example uses FFI bindings to GCD's dispatch_source_timer API for microsecond-precision
//! timing on macOS. GCD timers are the native high-resolution timer mechanism on macOS,
//! used by system frameworks and applications requiring precise timing.

use clap::Parser;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// GCD type definitions
#[repr(C)]
struct dispatch_object_s {
    _private: [u8; 0],
}

type dispatch_object_t = *mut dispatch_object_s;
type dispatch_source_t = dispatch_object_t;
type dispatch_queue_t = dispatch_object_t;
type dispatch_time_t = u64;

// GCD constants
const DISPATCH_TIME_NOW: dispatch_time_t = 0;

// GCD FFI bindings
#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> dispatch_queue_t;
    fn dispatch_source_create(
        type_: *const c_void,
        handle: usize,
        mask: usize,
        queue: dispatch_queue_t,
    ) -> dispatch_source_t;
    fn dispatch_source_set_timer(
        source: dispatch_source_t,
        start: dispatch_time_t,
        interval: u64,
        leeway: u64,
    );
    fn dispatch_source_set_event_handler_f(source: dispatch_source_t, handler: extern "C" fn());
    fn dispatch_resume(object: dispatch_object_t);
    fn dispatch_source_cancel(object: dispatch_object_t);
    fn dispatch_release(object: dispatch_object_t);
    static _dispatch_source_type_timer: c_void;
}

/// Application context for timing statistics
struct AppCtx {
    exact: AtomicU64,
    early: AtomicU64,
    late: AtomicU64,
    last_tick: Option<Instant>,
    interval_micros: u64,
    tolerance: Duration,
}

impl AppCtx {
    pub fn new(interval_micros: u64, tolerance: Duration) -> Self {
        Self {
            exact: AtomicU64::new(0),
            early: AtomicU64::new(0),
            late: AtomicU64::new(0),
            last_tick: None,
            interval_micros,
            tolerance,
        }
    }

    /// Record a tick and categorize its timing accuracy
    pub fn tick(&mut self) {
        let now = Instant::now();

        if let Some(last) = self.last_tick {
            let actual_interval = now - last;
            let expected_micros = self.interval_micros as i64;
            let actual_micros = actual_interval.as_micros() as i64;
            let diff_micros = actual_micros - expected_micros;
            let tolerance_micros = self.tolerance.as_micros() as i64;

            if diff_micros < -tolerance_micros {
                self.early.fetch_add(1, Ordering::SeqCst);
            } else if diff_micros > tolerance_micros {
                self.late.fetch_add(1, Ordering::SeqCst);
            } else {
                self.exact.fetch_add(1, Ordering::SeqCst);
            }
        }

        self.last_tick = Some(now);
    }

    pub fn exact_count(&self) -> u64 {
        self.exact.load(Ordering::SeqCst)
    }

    pub fn early_count(&self) -> u64 {
        self.early.load(Ordering::SeqCst)
    }

    pub fn late_count(&self) -> u64 {
        self.late.load(Ordering::SeqCst)
    }

    pub fn total_count(&self) -> u64 {
        self.exact_count() + self.early_count() + self.late_count()
    }
}

// Global context pointer for the C callback
static mut GLOBAL_CTX: Option<*mut AppCtx> = None;

// Timer callback - called by GCD on each timer fire
extern "C" fn timer_callback() {
    unsafe {
        if let Some(ctx_ptr) = GLOBAL_CTX {
            (*ctx_ptr).tick();
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "gcd_timer_accuracy")]
#[command(about = "High-resolution timer accuracy test using macOS GCD timers")]
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

fn fps_to_interval_micros(fps: u32) -> u64 {
    (1_000_000.0 / fps as f64) as u64
}

fn main() {
    let args = Args::parse();

    let interval_micros = fps_to_interval_micros(args.fps);
    let interval = Duration::from_micros(interval_micros);
    let tolerance = Duration::from_millis(args.tolerance);
    let test_duration = Duration::from_secs(args.duration);

    println!("GCD Timer Accuracy Test (macOS)");
    println!("================================");
    println!(
        "Target FPS: {} ({:.2} ms per frame)",
        args.fps,
        interval_micros as f64 / 1000.0
    );
    println!("Interval: {:?}", interval);
    println!("Tolerance: ±{} ms", args.tolerance);
    println!("Duration: {} seconds", args.duration);
    println!();

    // Create the context on the heap so it lives for the duration
    let mut ctx = Box::new(AppCtx::new(interval_micros, tolerance));

    // Store pointer in global for the callback
    unsafe {
        GLOBAL_CTX = Some(ctx.as_mut());
    }

    // Create GCD timer
    let source = unsafe {
        let queue = dispatch_get_global_queue(0, 0);
        let timer_type: *const c_void = &raw const _dispatch_source_type_timer;

        let source = dispatch_source_create(timer_type, 0, 0, queue);

        if source.is_null() {
            panic!("Failed to create dispatch source timer");
        }

        let interval_ns = interval.as_nanos() as u64;
        dispatch_source_set_timer(source, DISPATCH_TIME_NOW, interval_ns, 0);
        dispatch_source_set_event_handler_f(source, timer_callback);
        dispatch_resume(source);

        source
    };

    // Wait for the test duration
    std::thread::sleep(test_duration);

    // Stop the timer
    unsafe {
        dispatch_source_cancel(source);
        dispatch_release(source);
        GLOBAL_CTX = None;
    }

    // Collect and display results
    let exact = ctx.exact_count();
    let early = ctx.early_count();
    let late = ctx.late_count();
    let total = ctx.total_count();

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
