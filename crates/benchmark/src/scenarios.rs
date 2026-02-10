use std::time::Duration;

/// Configuration for a single timer in a benchmark scenario
#[derive(Debug, Clone)]
pub struct TimerConfig {
    /// Timer period (interval between executions)
    pub period: Duration,
    /// Number of timers with this configuration
    pub count: usize,
    /// Acceptable window before ideal execution time
    pub window_before: Duration,
    /// Acceptable window after ideal execution time
    pub window_after: Duration,
}

impl TimerConfig {
    pub fn new(
        period: Duration,
        count: usize,
        window_before: Duration,
        window_after: Duration,
    ) -> Self {
        Self {
            period,
            count,
            window_before,
            window_after,
        }
    }
}

/// A pre-defined benchmark scenario with multiple timers
#[derive(Debug, Clone)]
pub struct BenchmarkScenario {
    /// Scenario name
    pub name: &'static str,
    /// Description of what this scenario tests
    pub description: &'static str,
    /// Total duration to run the benchmark
    pub duration: Duration,
    /// Timer configurations for this scenario
    pub timers: Vec<TimerConfig>,
}

impl BenchmarkScenario {
    /// Retrieve a pre-defined scenario by name
    pub fn get(name: &str) -> Option<Self> {
        match name {
            "light" => Some(Self::light()),
            "medium" => Some(Self::medium()),
            "heavy" => Some(Self::heavy()),
            "extreme" => Some(Self::extreme()),
            "mixed-frequency" => Some(Self::mixed_frequency()),
            "burst" => Some(Self::burst()),
            "coalescing-test" => Some(Self::coalescing_test()),
            _ => None,
        }
    }

    /// Light load: Single device coordination scenario with 5 timers @ 1s
    pub fn light() -> Self {
        Self {
            name: "light",
            description: "Single device coordination (5 timers @ 1s)",
            duration: Duration::from_secs(30),
            timers: vec![TimerConfig::new(
                Duration::from_secs(1),
                5,
                Duration::from_millis(50),
                Duration::from_millis(50),
            )],
        }
    }

    /// Medium load: Mixed timer frequencies
    pub fn medium() -> Self {
        Self {
            name: "medium",
            description: "Mixed load (10@1s, 10@500ms, 5@100ms)",
            duration: Duration::from_secs(60),
            timers: vec![
                TimerConfig::new(
                    Duration::from_secs(1),
                    10,
                    Duration::from_millis(50),
                    Duration::from_millis(50),
                ),
                TimerConfig::new(
                    Duration::from_millis(500),
                    10,
                    Duration::from_millis(25),
                    Duration::from_millis(25),
                ),
                TimerConfig::new(
                    Duration::from_millis(100),
                    5,
                    Duration::from_millis(10),
                    Duration::from_millis(10),
                ),
            ],
        }
    }

    /// Heavy load: High-frequency timers
    pub fn heavy() -> Self {
        Self {
            name: "heavy",
            description: "High-frequency load (50@100ms, 30@50ms, 20@10ms)",
            duration: Duration::from_secs(120),
            timers: vec![
                TimerConfig::new(
                    Duration::from_millis(100),
                    50,
                    Duration::from_millis(10),
                    Duration::from_millis(10),
                ),
                TimerConfig::new(
                    Duration::from_millis(50),
                    30,
                    Duration::from_millis(5),
                    Duration::from_millis(5),
                ),
                TimerConfig::new(
                    Duration::from_millis(10),
                    20,
                    Duration::from_millis(2),
                    Duration::from_millis(2),
                ),
            ],
        }
    }

    /// Extreme load: Ramp-up stress test with very high-frequency timers
    pub fn extreme() -> Self {
        Self {
            name: "extreme",
            description: "Ramp-up stress test (10 @ 1ms, designed for gradual increase)",
            duration: Duration::from_secs(180),
            timers: vec![TimerConfig::new(
                Duration::from_millis(1),
                10,
                Duration::from_micros(500),
                Duration::from_micros(500),
            )],
        }
    }

    /// Mixed frequency: Wide variety of timer periods
    pub fn mixed_frequency() -> Self {
        Self {
            name: "mixed-frequency",
            description: "Wide variety of periods (5s down to 10ms)",
            duration: Duration::from_secs(90),
            timers: vec![
                TimerConfig::new(
                    Duration::from_secs(5),
                    3,
                    Duration::from_millis(100),
                    Duration::from_millis(100),
                ),
                TimerConfig::new(
                    Duration::from_secs(2),
                    5,
                    Duration::from_millis(50),
                    Duration::from_millis(50),
                ),
                TimerConfig::new(
                    Duration::from_secs(1),
                    10,
                    Duration::from_millis(50),
                    Duration::from_millis(50),
                ),
                TimerConfig::new(
                    Duration::from_millis(500),
                    15,
                    Duration::from_millis(25),
                    Duration::from_millis(25),
                ),
                TimerConfig::new(
                    Duration::from_millis(200),
                    10,
                    Duration::from_millis(10),
                    Duration::from_millis(10),
                ),
                TimerConfig::new(
                    Duration::from_millis(100),
                    8,
                    Duration::from_millis(10),
                    Duration::from_millis(10),
                ),
                TimerConfig::new(
                    Duration::from_millis(50),
                    5,
                    Duration::from_millis(5),
                    Duration::from_millis(5),
                ),
                TimerConfig::new(
                    Duration::from_millis(10),
                    4,
                    Duration::from_millis(2),
                    Duration::from_millis(2),
                ),
            ],
        }
    }

    /// Burst scenario: Short bursts of many timers
    pub fn burst() -> Self {
        Self {
            name: "burst",
            description: "Short bursts (100 timers @ 50ms)",
            duration: Duration::from_secs(45),
            timers: vec![TimerConfig::new(
                Duration::from_millis(50),
                100,
                Duration::from_millis(5),
                Duration::from_millis(5),
            )],
        }
    }

    /// Coalescing test: Many timers with aligned periods to test coalescing
    pub fn coalescing_test() -> Self {
        Self {
            name: "coalescing-test",
            description: "Timer coalescing test (many aligned timers)",
            duration: Duration::from_secs(60),
            timers: vec![
                // 20 timers at exactly 100ms - should coalesce well
                TimerConfig::new(
                    Duration::from_millis(100),
                    20,
                    Duration::from_millis(10),
                    Duration::from_millis(10),
                ),
                // 20 timers at exactly 200ms - should coalesce with each other
                TimerConfig::new(
                    Duration::from_millis(200),
                    20,
                    Duration::from_millis(10),
                    Duration::from_millis(10),
                ),
                // 10 timers at exactly 50ms - highest frequency
                TimerConfig::new(
                    Duration::from_millis(50),
                    10,
                    Duration::from_millis(5),
                    Duration::from_millis(5),
                ),
            ],
        }
    }

    /// Get the total number of timers in this scenario
    pub fn total_timers(&self) -> usize {
        self.timers.iter().map(|t| t.count).sum()
    }
}

/// Acceptance criteria for benchmark validation
#[derive(Debug, Clone)]
pub struct AcceptanceCriteria {
    /// Maximum acceptable average drift in microseconds
    pub max_avg_drift_us: f64,
    /// Maximum acceptable 99th percentile drift in microseconds
    pub max_p99_drift_us: f64,
    /// Minimum percentage of executions that should be "on time" (within window)
    pub min_on_time_percent: f64,
    /// Maximum percentage of missed timer executions
    pub max_missed_percent: f64,
    /// Maximum CPU usage percentage
    pub max_cpu_percent: f64,
}

impl AcceptanceCriteria {
    /// Get acceptance criteria appropriate for the given scenario
    pub fn for_scenario(name: &str) -> Self {
        match name {
            "light" => Self {
                max_avg_drift_us: 100.0,   // Very strict - low load should be precise
                max_p99_drift_us: 500.0,   // Tight p99 bound
                min_on_time_percent: 99.0, // Nearly all should be on time
                max_missed_percent: 0.1,   // Almost no misses allowed
                max_cpu_percent: 5.0,      // Very low CPU usage expected
            },
            "medium" => Self {
                max_avg_drift_us: 200.0,   // Moderate precision
                max_p99_drift_us: 1000.0,  // 1ms p99
                min_on_time_percent: 95.0, // Most should be on time
                max_missed_percent: 1.0,   // Few misses allowed
                max_cpu_percent: 15.0,     // Moderate CPU usage
            },
            "heavy" => Self {
                max_avg_drift_us: 500.0,   // Higher drift acceptable
                max_p99_drift_us: 2000.0,  // 2ms p99
                min_on_time_percent: 90.0, // Good portion on time
                max_missed_percent: 3.0,   // Some misses acceptable
                max_cpu_percent: 35.0,     // Higher CPU usage
            },
            "extreme" => Self {
                max_avg_drift_us: 1000.0,  // Very permissive
                max_p99_drift_us: 5000.0,  // 5ms p99
                min_on_time_percent: 80.0, // Lower expectations
                max_missed_percent: 10.0,  // Many misses acceptable
                max_cpu_percent: 60.0,     // High CPU usage expected
            },
            "mixed-frequency" => Self {
                max_avg_drift_us: 300.0,   // Moderate average
                max_p99_drift_us: 1500.0,  // 1.5ms p99
                min_on_time_percent: 92.0, // Good performance
                max_missed_percent: 2.0,   // Low miss rate
                max_cpu_percent: 25.0,     // Reasonable CPU
            },
            "burst" => Self {
                max_avg_drift_us: 400.0,   // Bursts can cause drift
                max_p99_drift_us: 2000.0,  // 2ms p99
                min_on_time_percent: 88.0, // Some degradation expected
                max_missed_percent: 5.0,   // Bursts may cause misses
                max_cpu_percent: 40.0,     // High CPU during bursts
            },
            "coalescing-test" => Self {
                max_avg_drift_us: 250.0,   // Should be efficient
                max_p99_drift_us: 1200.0,  // Good p99
                min_on_time_percent: 93.0, // High success rate
                max_missed_percent: 1.5,   // Low miss rate
                max_cpu_percent: 20.0,     // Coalescing should reduce CPU
            },
            _ => Self::default(), // Fallback to default
        }
    }

    /// Default acceptance criteria (moderate)
    pub fn default() -> Self {
        Self {
            max_avg_drift_us: 500.0,
            max_p99_drift_us: 2000.0,
            min_on_time_percent: 90.0,
            max_missed_percent: 5.0,
            max_cpu_percent: 30.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_lookup() {
        assert!(BenchmarkScenario::get("light").is_some());
        assert!(BenchmarkScenario::get("medium").is_some());
        assert!(BenchmarkScenario::get("heavy").is_some());
        assert!(BenchmarkScenario::get("extreme").is_some());
        assert!(BenchmarkScenario::get("mixed-frequency").is_some());
        assert!(BenchmarkScenario::get("burst").is_some());
        assert!(BenchmarkScenario::get("coalescing-test").is_some());
        assert!(BenchmarkScenario::get("nonexistent").is_none());
    }

    #[test]
    fn test_light_scenario() {
        let scenario = BenchmarkScenario::light();
        assert_eq!(scenario.name, "light");
        assert_eq!(scenario.duration, Duration::from_secs(30));
        assert_eq!(scenario.total_timers(), 5);
    }

    #[test]
    fn test_medium_scenario() {
        let scenario = BenchmarkScenario::medium();
        assert_eq!(scenario.name, "medium");
        assert_eq!(scenario.duration, Duration::from_secs(60));
        assert_eq!(scenario.total_timers(), 25); // 10 + 10 + 5
    }

    #[test]
    fn test_heavy_scenario() {
        let scenario = BenchmarkScenario::heavy();
        assert_eq!(scenario.name, "heavy");
        assert_eq!(scenario.duration, Duration::from_secs(120));
        assert_eq!(scenario.total_timers(), 100); // 50 + 30 + 20
    }

    #[test]
    fn test_extreme_scenario() {
        let scenario = BenchmarkScenario::extreme();
        assert_eq!(scenario.name, "extreme");
        assert_eq!(scenario.duration, Duration::from_secs(180));
        assert_eq!(scenario.total_timers(), 10);
    }

    #[test]
    fn test_acceptance_criteria() {
        let light_criteria = AcceptanceCriteria::for_scenario("light");
        let extreme_criteria = AcceptanceCriteria::for_scenario("extreme");

        // Light should be stricter than extreme
        assert!(light_criteria.max_avg_drift_us < extreme_criteria.max_avg_drift_us);
        assert!(light_criteria.max_p99_drift_us < extreme_criteria.max_p99_drift_us);
        assert!(light_criteria.min_on_time_percent > extreme_criteria.min_on_time_percent);
        assert!(light_criteria.max_missed_percent < extreme_criteria.max_missed_percent);
        assert!(light_criteria.max_cpu_percent < extreme_criteria.max_cpu_percent);
    }

    #[test]
    fn test_mixed_frequency_scenario() {
        let scenario = BenchmarkScenario::mixed_frequency();
        assert_eq!(scenario.name, "mixed-frequency");
        assert_eq!(scenario.total_timers(), 60); // 3+5+10+15+10+8+5+4
    }

    #[test]
    fn test_burst_scenario() {
        let scenario = BenchmarkScenario::burst();
        assert_eq!(scenario.name, "burst");
        assert_eq!(scenario.total_timers(), 100);
    }

    #[test]
    fn test_coalescing_scenario() {
        let scenario = BenchmarkScenario::coalescing_test();
        assert_eq!(scenario.name, "coalescing-test");
        assert_eq!(scenario.total_timers(), 50); // 20 + 20 + 10
    }
}
