use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use crate::metrics::BenchmarkResults;

// macOS platform benchmark using thread-based timers with sleep
// This approximates what GCD would do internally - using kqueue/kevent for timer events

pub async fn run_benchmark(
    duration: Duration,
    period: Duration,
    num_timers: usize,
    window_before: Duration,
    window_after: Duration,
) -> BenchmarkResults {
    let timestamps: Arc<Mutex<Vec<(usize, Instant, Instant)>>> = Arc::new(Mutex::new(Vec::new()));
    let missed_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let execution_counts: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(vec![0; num_timers]));
    let wakeup_count = Arc::new(AtomicU64::new(0));
    let scheduler_overhead_ns = Arc::new(AtomicU64::new(0));
    let task_execution_ns = Arc::new(AtomicU64::new(0));

    let start_cpu = get_thread_cpu_time();
    let start_time = Instant::now();
    let report_interval = Duration::from_secs(1);
    let last_report = Arc::new(Mutex::new(start_time));
    let stop_flag = Arc::new(AtomicBool::new(false));

    println!("Starting benchmark...\n");

    // Spawn timer threads (each represents a GCD timer source)
    let mut timer_handles = Vec::new();

    for timer_id in 0..num_timers {
        let timestamps_clone = Arc::clone(&timestamps);
        let missed_count_clone = Arc::clone(&missed_count);
        let execution_counts_clone = Arc::clone(&execution_counts);
        let last_report_clone = Arc::clone(&last_report);
        let stop_flag_clone = Arc::clone(&stop_flag);
        let wakeup_count_clone = Arc::clone(&wakeup_count);
        let scheduler_overhead_ns_clone = Arc::clone(&scheduler_overhead_ns);
        let task_execution_ns_clone = Arc::clone(&task_execution_ns);

        let handle = std::thread::spawn(move || {
            let mut count = 0;

            loop {
                // Sleep until next period (this is what GCD timer does internally)
                let scheduler_start = Instant::now();
                std::thread::sleep(period);
                let wakeup_time = Instant::now();

                // Track scheduler overhead (wakeup latency beyond requested period)
                let actual_sleep = wakeup_time.duration_since(scheduler_start);
                let latency = actual_sleep.saturating_sub(period);
                scheduler_overhead_ns_clone.fetch_add(
                    latency.as_nanos() as u64,
                    Ordering::Relaxed
                );
                wakeup_count_clone.fetch_add(1, Ordering::Relaxed);

                // Task execution starts
                let task_start = Instant::now();

                let now = Instant::now();
                let elapsed = now.duration_since(start_time);
                count += 1;

                // Calculate expected execution count based on elapsed time
                let expected_count = (elapsed.as_nanos() / period.as_nanos()) as usize;

                // Track actual execution count for this timer
                let mut exec_counts = execution_counts_clone.lock().unwrap();
                let prev_count = exec_counts[timer_id];
                exec_counts[timer_id] = expected_count;

                // Detect missed executions
                if expected_count > prev_count + 1 && prev_count > 0 {
                    let missed = expected_count - prev_count - 1;
                    *missed_count_clone.lock().unwrap() += missed;
                }
                drop(exec_counts);

                // Record timestamp with ideal time
                let ideal_time = start_time + period * count;
                timestamps_clone.lock().unwrap().push((timer_id, now, ideal_time));

                // Track task execution time
                let task_end = Instant::now();
                task_execution_ns_clone.fetch_add(
                    task_end.duration_since(task_start).as_nanos() as u64,
                    Ordering::Relaxed
                );

                // Periodic progress report (only from timer 0 to avoid spam)
                if timer_id == 0 {
                    let mut last_report_locked = last_report_clone.lock().unwrap();
                    if now.duration_since(*last_report_locked) >= report_interval {
                        let execs = timestamps_clone.lock().unwrap().len();
                        let missed = *missed_count_clone.lock().unwrap();
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
                        *last_report_locked += report_interval;
                    }
                }

                // Check stop flag AFTER executing (so we don't skip the final execution)
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        timer_handles.push(handle);
    }

    // Wait for the benchmark duration
    tokio::time::sleep(duration).await;

    // Stop all timer threads
    stop_flag.store(true, Ordering::Relaxed);

    // Wait for all threads to finish
    for handle in timer_handles {
        let _ = handle.join();
    }

    let final_execs = timestamps.lock().unwrap().len();
    let final_missed = *missed_count.lock().unwrap();
    let elapsed = start_time.elapsed();

    println!(
        "\n[{:>5.1}s] Benchmark complete! Executions: {}  Missed: {}",
        elapsed.as_secs_f64(),
        final_execs,
        final_missed
    );

    let end_cpu = get_thread_cpu_time();

    BenchmarkResults::from_timestamps(
        timestamps.lock().unwrap().clone(),
        final_missed,
        end_cpu - start_cpu,
        0, // memory_kb - TODO: implement
        0, // context_switches - TODO: implement
        period,
        duration,
        window_before,
        window_after,
        wakeup_count.load(Ordering::Relaxed),
        Duration::from_nanos(scheduler_overhead_ns.load(Ordering::Relaxed)),
        Duration::from_nanos(task_execution_ns.load(Ordering::Relaxed)),
    )
}

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
        
        // THREAD_BASIC_INFO = 3
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
