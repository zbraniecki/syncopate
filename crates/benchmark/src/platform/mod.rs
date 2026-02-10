use crate::metrics::BenchmarkResults;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

/// Run benchmark using native platform timers
pub async fn run_benchmark(
    duration: Duration,
    period: Duration,
    num_timers: usize,
    window_before: Duration,
    window_after: Duration,
) -> BenchmarkResults {
    #[cfg(target_os = "macos")]
    {
        macos::run_benchmark(duration, period, num_timers, window_before, window_after).await
    }
    #[cfg(target_os = "linux")]
    {
        linux::run_benchmark(duration, period, num_timers, window_before, window_after).await
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        panic!("Platform not supported. Only macOS and Linux are supported.");
    }
}
