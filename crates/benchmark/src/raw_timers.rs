use crate::metrics::BenchmarkResults;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct TimerContext {
    timestamps: Arc<Mutex<Vec<(usize, Instant, Instant)>>>, // (timer_id, actual, ideal)
    wakeup_count: Arc<AtomicU64>,
}

/// Run benchmark using raw std::thread::sleep (single-threaded)
pub fn run_std_sleep_benchmark(
    duration: Duration,
    period: Duration,
    window_before: Duration,
    window_after: Duration,
) -> BenchmarkResults {
    let ctx = TimerContext {
        timestamps: Arc::new(Mutex::new(Vec::new())),
        wakeup_count: Arc::new(AtomicU64::new(0)),
    };

    let benchmark_start = Instant::now();
    let start_cpu = get_thread_cpu_time();
    let mut next_tick = benchmark_start + period;

    println!("Starting raw std::thread::sleep benchmark...\n");

    loop {
        let now = Instant::now();

        if now >= next_tick {
            let ideal_time = next_tick;
            let actual_time = now;

            ctx.timestamps
                .lock()
                .unwrap()
                .push((0, actual_time, ideal_time));

            next_tick = now + period;
        }

        if benchmark_start.elapsed() >= duration {
            break;
        }

        // Sleep until next tick (with small buffer to avoid oversleeping)
        let now = Instant::now();
        if now < next_tick {
            let sleep_duration = next_tick.duration_since(now);
            if sleep_duration > Duration::from_micros(100) {
                ctx.wakeup_count.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(sleep_duration.saturating_sub(Duration::from_micros(50)));
            }
        }
    }

    let actual_duration = benchmark_start.elapsed();
    let end_cpu = get_thread_cpu_time();
    let cpu_time = Duration::from_nanos(end_cpu.saturating_sub(start_cpu));

    let timestamps = ctx.timestamps.lock().unwrap().clone();
    let wakeup_count = ctx.wakeup_count.load(Ordering::Relaxed);

    BenchmarkResults::from_timestamps(
        timestamps,
        0, // missed_count
        cpu_time,
        0, // memory_kb
        0, // context_switches
        period,
        actual_duration,
        window_before,
        window_after,
        wakeup_count,
        Duration::ZERO, // scheduler_overhead
        Duration::ZERO, // task_execution_duration
    )
}

/// Run benchmark using raw tokio::time::sleep (async, single timer)
pub async fn run_tokio_sleep_benchmark(
    duration: Duration,
    period: Duration,
    window_before: Duration,
    window_after: Duration,
) -> BenchmarkResults {
    let ctx = TimerContext {
        timestamps: Arc::new(Mutex::new(Vec::new())),
        wakeup_count: Arc::new(AtomicU64::new(0)),
    };

    let benchmark_start = Instant::now();
    let start_cpu = get_thread_cpu_time();
    let mut next_tick = benchmark_start + period;

    println!("Starting raw tokio::time::sleep benchmark...\n");

    loop {
        let now = Instant::now();

        if now >= next_tick {
            let ideal_time = next_tick;
            let actual_time = now;

            ctx.timestamps
                .lock()
                .unwrap()
                .push((0, actual_time, ideal_time));

            next_tick = now + period;
        }

        if benchmark_start.elapsed() >= duration {
            break;
        }

        // Sleep until next tick
        let now = Instant::now();
        if now < next_tick {
            let sleep_duration = next_tick.duration_since(now);
            if sleep_duration > Duration::from_micros(100) {
                ctx.wakeup_count.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(sleep_duration.saturating_sub(Duration::from_micros(50))).await;
            }
        }
    }

    let actual_duration = benchmark_start.elapsed();
    let end_cpu = get_thread_cpu_time();
    let cpu_time = Duration::from_nanos(end_cpu.saturating_sub(start_cpu));

    let timestamps = ctx.timestamps.lock().unwrap().clone();
    let wakeup_count = ctx.wakeup_count.load(Ordering::Relaxed);

    BenchmarkResults::from_timestamps(
        timestamps,
        0, // missed_count
        cpu_time,
        0, // memory_kb
        0, // context_switches
        period,
        actual_duration,
        window_before,
        window_after,
        wakeup_count,
        Duration::ZERO, // scheduler_overhead
        Duration::ZERO, // task_execution_duration
    )
}

// Platform-specific CPU time measurement
#[cfg(target_os = "macos")]
fn get_thread_cpu_time() -> u64 {
    use libc::{THREAD_BASIC_INFO, mach_thread_self, thread_basic_info, thread_info};
    use std::mem;

    unsafe {
        let mut info: thread_basic_info = mem::zeroed();
        let mut count = (mem::size_of::<thread_basic_info>() / mem::size_of::<i32>()) as u32;

        let result = thread_info(
            mach_thread_self(),
            THREAD_BASIC_INFO as u32,
            &mut info as *mut _ as *mut i32,
            &mut count,
        );

        if result == 0 {
            let user_ns = info.user_time.seconds as u64 * 1_000_000_000
                + info.user_time.microseconds as u64 * 1_000;
            let system_ns = info.system_time.seconds as u64 * 1_000_000_000
                + info.system_time.microseconds as u64 * 1_000;
            user_ns + system_ns
        } else {
            0
        }
    }
}

#[cfg(target_os = "linux")]
fn get_thread_cpu_time() -> u64 {
    use nix::sys::time::{ClockId, clock_gettime};

    match clock_gettime(ClockId::CLOCK_THREAD_CPUTIME_ID) {
        Ok(timespec) => (timespec.tv_sec() as u64 * 1_000_000_000) + timespec.tv_nsec() as u64,
        Err(_) => 0,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_thread_cpu_time() -> u64 {
    // Fallback for unsupported platforms
    0
}
