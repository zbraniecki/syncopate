use crate::metrics::BenchmarkResults;

/// Winner of a statistical comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winner {
    Syncopate,
    System,
    Tie,
}

/// Result of comparing two benchmark results on a single metric
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    pub metric_name: &'static str,
    pub syncopate_value: f64,
    pub system_value: f64,
    #[allow(dead_code)]
    pub p_value: f64,
    #[allow(dead_code)]
    pub effect_size: f64, // Cohen's d
    pub winner: Winner,
}

/// Perform Welch's t-test (doesn't assume equal variance)
/// Returns (t_statistic, p_value)
#[cfg(test)]
pub fn welch_t_test(sample1: &[f64], sample2: &[f64]) -> (f64, f64) {
    use statrs::distribution::{ContinuousCDF, StudentsT};
    if sample1.is_empty() || sample2.is_empty() {
        return (0.0, 1.0); // No significant difference if no data
    }

    let n1 = sample1.len() as f64;
    let n2 = sample2.len() as f64;

    // Calculate means
    let mean1 = sample1.iter().sum::<f64>() / n1;
    let mean2 = sample2.iter().sum::<f64>() / n2;

    // Calculate variances
    let var1 = sample1
        .iter()
        .map(|&x| {
            let diff = x - mean1;
            diff * diff
        })
        .sum::<f64>()
        / (n1 - 1.0);

    let var2 = sample2
        .iter()
        .map(|&x| {
            let diff = x - mean2;
            diff * diff
        })
        .sum::<f64>()
        / (n2 - 1.0);

    // Welch's t-statistic
    let t = (mean1 - mean2) / ((var1 / n1) + (var2 / n2)).sqrt();

    // Welch-Satterthwaite degrees of freedom
    let numerator = ((var1 / n1) + (var2 / n2)).powi(2);
    let denominator = (var1 / n1).powi(2) / (n1 - 1.0) + (var2 / n2).powi(2) / (n2 - 1.0);
    let df = numerator / denominator;

    // Calculate p-value using t-distribution
    // Handle edge cases where df might be invalid
    if !df.is_finite() || df <= 0.0 || t.is_nan() {
        return (0.0, 1.0); // No significant difference if params are invalid
    }

    match StudentsT::new(0.0, 1.0, df) {
        Ok(t_dist) => {
            let p_value = 2.0 * (1.0 - t_dist.cdf(t.abs()));
            (t, p_value)
        }
        Err(_) => (0.0, 1.0), // Failed to create distribution, no significant difference
    }
}

/// Calculate Cohen's d effect size
/// Interpretation: |d| > 0.8 = large, 0.5-0.8 = medium, < 0.5 = small
#[cfg(test)]
pub fn cohens_d(sample1: &[f64], sample2: &[f64]) -> f64 {
    if sample1.is_empty() || sample2.is_empty() {
        return 0.0;
    }

    let n1 = sample1.len() as f64;
    let n2 = sample2.len() as f64;

    // Calculate means
    let mean1 = sample1.iter().sum::<f64>() / n1;
    let mean2 = sample2.iter().sum::<f64>() / n2;

    // Calculate standard deviations
    let var1 = sample1
        .iter()
        .map(|&x| {
            let diff = x - mean1;
            diff * diff
        })
        .sum::<f64>()
        / (n1 - 1.0);

    let var2 = sample2
        .iter()
        .map(|&x| {
            let diff = x - mean2;
            diff * diff
        })
        .sum::<f64>()
        / (n2 - 1.0);

    // Pooled standard deviation
    let pooled_sd = (((n1 - 1.0) * var1 + (n2 - 1.0) * var2) / (n1 + n2 - 2.0)).sqrt();

    if pooled_sd == 0.0 {
        return 0.0;
    }

    (mean1 - mean2) / pooled_sd
}

/// Compare two benchmark results
/// Since we're comparing aggregate metrics (not distributions), we use simple comparison
/// Returns a list of comparison results for key metrics
pub fn compare_benchmarks(
    syncopate: &BenchmarkResults,
    system: &BenchmarkResults,
) -> Vec<ComparisonResult> {
    let mut results = Vec::new();

    // Helper to determine winner by comparing values (lower is better)
    let compare_lower_better = |syncopate_val: f64, system_val: f64| -> Winner {
        let diff_percent = ((syncopate_val - system_val) / system_val).abs() * 100.0;
        if diff_percent > 5.0 {
            // More than 5% difference
            if syncopate_val < system_val {
                Winner::Syncopate
            } else {
                Winner::System
            }
        } else {
            Winner::Tie
        }
    };

    // 1. Average drift (lower absolute value is better - closer to ideal timing)
    let winner = compare_lower_better(syncopate.avg_drift.abs(), system.avg_drift.abs());

    results.push(ComparisonResult {
        metric_name: "avg_drift",
        syncopate_value: syncopate.avg_drift.abs(),
        system_value: system.avg_drift.abs(),
        p_value: 0.0,     // Not applicable for single aggregate values
        effect_size: 0.0, // Not applicable for single aggregate values
        winner,
    });

    // 2. P99 drift (lower is better - better tail latency)
    let winner = compare_lower_better(syncopate.p99_drift.abs(), system.p99_drift.abs());

    results.push(ComparisonResult {
        metric_name: "p99_drift",
        syncopate_value: syncopate.p99_drift.abs(),
        system_value: system.p99_drift.abs(),
        p_value: 0.0,
        effect_size: 0.0,
        winner,
    });

    // 3. Jitter (standard deviation - lower is better for consistency)
    let winner = compare_lower_better(syncopate.stddev_drift, system.stddev_drift);

    results.push(ComparisonResult {
        metric_name: "jitter",
        syncopate_value: syncopate.stddev_drift,
        system_value: system.stddev_drift,
        p_value: 0.0,
        effect_size: 0.0,
        winner,
    });

    // 4. CPU percentage (lower is better - more efficient)
    let syncopate_cpu_percent = if syncopate.actual_duration_secs > 0.0 {
        (syncopate.cpu_time.as_secs_f64() / syncopate.actual_duration_secs) * 100.0
    } else {
        0.0
    };
    let system_cpu_percent = if system.actual_duration_secs > 0.0 {
        (system.cpu_time.as_secs_f64() / system.actual_duration_secs) * 100.0
    } else {
        0.0
    };

    let winner = compare_lower_better(syncopate_cpu_percent, system_cpu_percent);

    results.push(ComparisonResult {
        metric_name: "cpu_percent",
        syncopate_value: syncopate_cpu_percent,
        system_value: system_cpu_percent,
        p_value: 0.0,
        effect_size: 0.0,
        winner,
    });

    // 5. Scheduler overhead percentage (lower is better - less time in scheduler)
    let winner = compare_lower_better(
        syncopate.scheduler_overhead_percent,
        system.scheduler_overhead_percent,
    );

    results.push(ComparisonResult {
        metric_name: "scheduler_overhead_percent",
        syncopate_value: syncopate.scheduler_overhead_percent,
        system_value: system.scheduler_overhead_percent,
        p_value: 0.0,
        effect_size: 0.0,
        winner,
    });

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_welch_t_test_identical_samples() {
        let sample1 = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sample2 = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let (t, p) = welch_t_test(&sample1, &sample2);
        assert!(t.abs() < 0.001);
        assert!(p > 0.95); // Very high p-value for identical samples
    }

    #[test]
    fn test_welch_t_test_different_samples() {
        let sample1 = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sample2 = vec![10.0, 11.0, 12.0, 13.0, 14.0];
        let (t, p) = welch_t_test(&sample1, &sample2);
        assert!(t.abs() > 5.0); // Large t-statistic
        assert!(p < 0.05); // Significant difference
    }

    #[test]
    fn test_cohens_d_no_effect() {
        let sample1 = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sample2 = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let d = cohens_d(&sample1, &sample2);
        assert!(d.abs() < 0.001);
    }

    #[test]
    fn test_cohens_d_large_effect() {
        let sample1 = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sample2 = vec![10.0, 11.0, 12.0, 13.0, 14.0];
        let d = cohens_d(&sample1, &sample2);
        assert!(d.abs() > 2.0); // Very large effect size
    }

    #[test]
    fn test_cohens_d_empty_samples() {
        let sample1: Vec<f64> = vec![];
        let sample2 = vec![1.0, 2.0, 3.0];
        let d = cohens_d(&sample1, &sample2);
        assert_eq!(d, 0.0);
    }
}
