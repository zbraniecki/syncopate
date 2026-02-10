use crate::metrics::BenchmarkResults;
use nix::errno::Errno;
use nix::sys::epoll::{EpollEvent, EpollFlags, EpollOp, epoll_create, epoll_ctl, epoll_wait};
use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, TimerFd, TimerFlags, TimerSetTimeFlags};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub async fn run_benchmark(
    duration: Duration,
    period: Duration,
    num_timers: usize,
    window_before: Duration,
    window_after: Duration,
) -> BenchmarkResults {
    let timestamps: Arc<Mutex<Vec<(usize, Instant, Instant)>>> = Arc::new(Mutex::new(Vec::new()));
    let wakeup_count = Arc::new(AtomicU64::new(0));
    let scheduler_overhead_ns = Arc::new(AtomicU64::new(0));
    let task_execution_ns = Arc::new(AtomicU64::new(0));

    // Create epoll instance
    let epoll_fd = epoll_create().expect("Failed to create epoll");

    // Create timerfds
    let mut timers: Vec<(TimerFd, usize)> = Vec::new();
    let start_time = Instant::now();

    for i in 0..num_timers {
        let timer = TimerFd::new(
            ClockId::CLOCK_MONOTONIC,
            TimerFlags::TFD_NONBLOCK | TimerFlags::TFD_CLOEXEC,
        )
        .expect("Failed to create timerfd");

        // Set interval
        let itimerspec = nix::sys::timerfd::Itimerspec {
            it_interval: duration_to_timespec(period),
            it_value: duration_to_timespec(period), // First expiration
        };

        timer
            .set(&itimerspec, TimerSetTimeFlags::empty())
            .expect("Failed to set timer");

        // Add to epoll
        let mut event = EpollEvent::new(EpollFlags::EPOLLIN, i as u64);
        epoll_ctl(
            epoll_fd,
            EpollOp::EpollCtlAdd,
            timer.as_raw_fd(),
            Some(&mut event),
        )
        .expect("Failed to add timer to epoll");

        timers.push((timer, i));
    }

    let start_cpu = get_thread_cpu_time();
    let end_time = start_time + duration;
    let mut events: [EpollEvent; 1024] = [EpollEvent::empty(); 1024];

    while Instant::now() < end_time {
        let timeout_ms = 10i32; // 10ms timeout to check duration

        // Track epoll_wait() call and duration
        let poll_start = Instant::now();
        wakeup_count.fetch_add(1, Ordering::Relaxed);

        match epoll_wait(epoll_fd, &mut events, timeout_ms) {
            Ok(n) => {
                let poll_duration = poll_start.elapsed();
                scheduler_overhead_ns.fetch_add(poll_duration.as_nanos() as u64, Ordering::Relaxed);

                let now = Instant::now();
                for i in 0..n {
                    let timer_id = events[i].data() as usize;
                    if let Some((timer, _)) = timers.get(timer_id) {
                        // Read to clear the timer
                        let mut buf = [0u8; 8];
                        let _ = nix::unistd::read(timer.as_raw_fd(), &mut buf);
                        let expirations = u64::from_ne_bytes(buf);

                        // Record timestamps for each expiration
                        for j in 0..expirations {
                            let task_start = Instant::now();
                            let elapsed = now.duration_since(start_time);
                            let expected_count = (elapsed.as_nanos() / period.as_nanos()) as usize;
                            let ideal_time = start_time
                                + period * (expected_count - (expirations - j - 1) as usize) as u32;
                            timestamps.lock().unwrap().push((timer_id, now, ideal_time));
                            let task_duration = task_start.elapsed();
                            task_execution_ns
                                .fetch_add(task_duration.as_nanos() as u64, Ordering::Relaxed);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("epoll_wait error: {:?}", e);
                break;
            }
        }
    }

    let end_cpu = get_thread_cpu_time();

    // Cleanup
    for (timer, _) in &timers {
        let _ = epoll_ctl(epoll_fd, EpollOp::EpollCtlDel, timer.as_raw_fd(), None);
    }
    let _ = nix::unistd::close(epoll_fd);

    let memory_kb = get_memory_usage();
    let context_switches = get_context_switches();

    let wakeup_count_val = wakeup_count.load(Ordering::Relaxed);
    let scheduler_overhead = Duration::from_nanos(scheduler_overhead_ns.load(Ordering::Relaxed));
    let task_execution_duration = Duration::from_nanos(task_execution_ns.load(Ordering::Relaxed));

    BenchmarkResults::from_timestamps(
        timestamps.lock().unwrap().clone(),
        0, // TODO: Track missed ticks
        end_cpu - start_cpu,
        memory_kb,
        context_switches,
        period,
        duration,
        window_before,
        window_after,
        wakeup_count_val,
        scheduler_overhead,
        task_execution_duration,
    )
}

fn duration_to_timespec(d: Duration) -> TimeSpec {
    TimeSpec::new(d.as_secs() as i64, d.subsec_nanos() as i64)
}

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
