use crate::metrics::BenchmarkResults;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

/// Context for collecting timing data from callbacks
#[derive(Clone)]
struct BenchmarkContext {
    timestamps: Arc<Mutex<Vec<(usize, Instant, Instant)>>>, // (timer_id, actual, ideal)
    missed_count: Arc<Mutex<usize>>,
    wakeup_count: Arc<AtomicU64>,
    scheduler_overhead_ns: Arc<AtomicU64>,
    task_execution_ns: Arc<AtomicU64>,
}

#[allow(clippy::too_many_arguments)]
pub fn run_benchmark(
    duration: Duration,
    period: Duration,
    min_period: Duration,
    max_period: Duration,
    num_timers: usize,
    window_before: Duration,
    window_after: Duration,
    sleep_compensation: Duration,
) -> BenchmarkResults {
    let ctx = BenchmarkContext {
        timestamps: Arc::new(Mutex::new(Vec::new())),
        missed_count: Arc::new(Mutex::new(0)),
        wakeup_count: Arc::new(AtomicU64::new(0)),
        scheduler_overhead_ns: Arc::new(AtomicU64::new(0)),
        task_execution_ns: Arc::new(AtomicU64::new(0)),
    };

    // Build scheduler with context
    let (_handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(min_period)
        .max_period(max_period)
        .sleep_compensation(sleep_compensation)
        .with_context(ctx.clone())
        .build();

    // Add tasks BEFORE starting the scheduler loop to avoid startup race
    for i in 0..num_timers {
        let timestamps_clone = Arc::clone(&ctx.timestamps);
        let missed_clone = Arc::clone(&ctx.missed_count);
        let task_exec_ns = Arc::clone(&ctx.task_execution_ns);

        let config = TaskConfig {
            task_type: TaskType::Periodic {
                period,
                window_before,
                window_after,
                anchored: false,
            },
            priority: 0,
            name: Some(format!("timer_{}", i)),
            on_execute: None,
            on_miss: None,
        }
        .with_executor(move |exec, _ctx| {
            let task_start = Instant::now();
            timestamps_clone
                .lock()
                .unwrap()
                .push((i, exec.actual_time, exec.ideal_time));
            let task_duration = task_start.elapsed();
            task_exec_ns.fetch_add(task_duration.as_nanos() as u64, Ordering::Relaxed);
        })
        .with_miss_handler(move |_miss, _ctx| {
            *missed_clone.lock().unwrap() += 1;
        });

        scheduler
            .add_task_local(config)
            .expect("Failed to add task");
    }

    let benchmark_start = Instant::now();
    let start_cpu = get_thread_cpu_time();
    let report_interval = Duration::from_secs(1);
    let mut last_report = benchmark_start;

    println!("Starting syncopate+std::thread::sleep benchmark...\n");

    loop {
        // Track poll() call and duration
        let poll_start = Instant::now();
        ctx.wakeup_count.fetch_add(1, Ordering::Relaxed);

        let idle_duration = scheduler.poll();

        let poll_duration = poll_start.elapsed();
        ctx.scheduler_overhead_ns
            .fetch_add(poll_duration.as_nanos() as u64, Ordering::Relaxed);

        let now = Instant::now();

        // Periodic progress report
        if now.duration_since(last_report) >= report_interval {
            let elapsed = now.duration_since(benchmark_start);
            let execs = ctx.timestamps.lock().unwrap().len();
            let missed = *ctx.missed_count.lock().unwrap();
            let expected_sofar = (elapsed.as_nanos() / period.as_nanos()) as usize * num_timers;
            let idle_pct = if expected_sofar > 0 {
                (100.0 - ((execs as f64 / expected_sofar as f64) * 100.0)).max(0.0)
            } else {
                0.0
            };

            println!(
                "[{:>5.1}s] Executions: {:>6}  Missed: {:>4}  Idle: {:>6.1}%",
                elapsed.as_secs_f64(),
                execs,
                missed,
                idle_pct
            );
            last_report += report_interval;
        }

        // Check if benchmark is complete
        if benchmark_start.elapsed() >= duration {
            break;
        }

        // Sleep for idle duration using std::thread::sleep (but don't overshoot benchmark end time)
        if idle_duration > Duration::ZERO {
            // Cap sleep at remaining benchmark time to ensure prompt exit
            let time_remaining = duration.saturating_sub(benchmark_start.elapsed());
            let sleep_duration = idle_duration.min(time_remaining);

            if sleep_duration > Duration::ZERO {
                std::thread::sleep(sleep_duration);
            }
        }
    }

    let final_execs = ctx.timestamps.lock().unwrap().len();
    let final_missed = *ctx.missed_count.lock().unwrap();
    let elapsed = benchmark_start.elapsed();

    println!(
        "\n[{:>5.1}s] Benchmark complete! Executions: {}  Missed: {}",
        elapsed.as_secs_f64(),
        final_execs,
        final_missed
    );

    let end_cpu = get_thread_cpu_time();
    let cpu_time = end_cpu.saturating_sub(start_cpu);
    let memory_kb = get_memory_usage();
    let context_switches = get_context_switches();

    let wakeup_count = ctx.wakeup_count.load(Ordering::Relaxed);
    let scheduler_overhead =
        Duration::from_nanos(ctx.scheduler_overhead_ns.load(Ordering::Relaxed));
    let task_execution_duration =
        Duration::from_nanos(ctx.task_execution_ns.load(Ordering::Relaxed));

    let timestamps = ctx.timestamps.lock().unwrap().clone();

    BenchmarkResults::from_timestamps(
        timestamps,
        final_missed,
        cpu_time,
        memory_kb,
        context_switches,
        period,
        duration,
        window_before,
        window_after,
        wakeup_count,
        scheduler_overhead,
        task_execution_duration,
    )
}

#[cfg(target_os = "macos")]
fn get_thread_cpu_time() -> Duration {
    use std::os::raw::{c_int, c_uint};

    unsafe extern "C" {
        fn mach_thread_self() -> c_uint;
        fn thread_info(
            target_act: c_uint,
            flavor: c_int,
            thread_info_out: *mut c_int,
            thread_info_count: *mut c_uint,
        ) -> c_int;
    }

    unsafe {
        let thread = mach_thread_self();
        let mut info: [c_int; 10] = [0; 10];
        let mut count: c_uint = 10;

        let kr = thread_info(thread, 3, info.as_mut_ptr(), &mut count);
        if kr == 0 {
            let user_secs = info[0] as u64;
            let user_micros = info[1] as u64;
            let sys_secs = info[2] as u64;
            let sys_micros = info[3] as u64;

            Duration::from_secs(user_secs + sys_secs)
                + Duration::from_micros(user_micros + sys_micros)
        } else {
            Duration::ZERO
        }
    }
}

#[cfg(target_os = "linux")]
fn get_thread_cpu_time() -> Duration {
    use nix::sys::resource::{UsageWho, getrusage};

    match getrusage(UsageWho::RUSAGE_THREAD) {
        Ok(usage) => {
            let user = usage.user_time();
            let sys = usage.system_time();
            Duration::from_secs(user.tv_sec() as u64 + sys.tv_sec() as u64)
                + Duration::from_micros((user.tv_usec() + sys.tv_usec()) as u64)
        }
        Err(_) => Duration::ZERO,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_thread_cpu_time() -> Duration {
    Duration::ZERO
}

// Memory usage tracking
#[cfg(target_os = "macos")]
fn get_memory_usage() -> u64 {
    // macOS memory tracking
    0 // TODO: Implement mach task_info
}

#[cfg(target_os = "linux")]
fn get_memory_usage() -> u64 {
    use std::fs::read_to_string;

    if let Ok(status) = read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                // Parse "VmRSS:    1234 kB"
                let parts: Vec<_> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return kb;
                    }
                }
            }
        }
    }
    0
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_memory_usage() -> u64 {
    0
}

// Context switch tracking
#[cfg(target_os = "macos")]
fn get_context_switches() -> u64 {
    // macOS context switch tracking via thread_info
    0 // TODO: Implement
}

#[cfg(target_os = "linux")]
fn get_context_switches() -> u64 {
    use nix::sys::resource::{UsageWho, getrusage};

    match getrusage(UsageWho::RUSAGE_THREAD) {
        Ok(usage) => {
            // ru_nvcsw = voluntary context switches
            // ru_nivcsw = involuntary context switches
            usage.num_voluntary_context_switches() as u64
                + usage.num_involuntary_context_switches() as u64
        }
        Err(_) => 0,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_context_switches() -> u64 {
    0
}
