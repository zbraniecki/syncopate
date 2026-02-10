use crate::metrics::BenchmarkResults;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Write};

/// Configuration parameters for a benchmark run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub system: String,
    pub scenario: Option<String>,
    pub duration_secs: u64,
    pub timers: usize,
    pub task_period_us: u64,
}

/// JSON export structure combining config and results
#[derive(Debug, Serialize, Deserialize)]
struct JsonExport {
    config: BenchmarkConfig,
    results: JsonResults,
}

/// Serializable version of BenchmarkResults
#[derive(Debug, Serialize, Deserialize)]
struct JsonResults {
    // Execution summary
    total_executions: usize,
    early_count: usize,
    on_time_count: usize,
    late_count: usize,
    missed_count: usize,

    // Baseline offset (median raw delta from ideal)
    baseline_delta: i64,

    // Corrected jitter statistics (signed)
    min_drift: f64,
    max_drift: f64,
    avg_drift: f64,
    stddev_drift: f64,

    // Drift percentiles
    p50_drift: f64,
    p95_drift: f64,
    p99_drift: f64,

    // Expected vs actual metrics
    expected_executions: usize,
    expected_duration_secs: f64,
    actual_duration_secs: f64,

    // Resource usage
    cpu_time_secs: f64,
    memory_kb: u64,
    context_switches: u64,

    // Scheduler-specific metrics
    scheduler_overhead_percent: f64,
    coalescing_ratio: f64,
    wakeup_count: u64,
    avg_tasks_per_wakeup: f64,
    avg_task_execution_us: f64,

    // Outliers
    top_outliers: Vec<OutlierData>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OutlierData {
    execution_num: usize,
    drift: i64,
    category: String,
    notes: String,
}

/// Export benchmark results to JSON format
///
/// Creates a JSON file with complete benchmark configuration and results.
/// The output is pretty-printed for readability.
///
/// # Arguments
/// * `results` - The benchmark results to export
/// * `config` - The benchmark configuration
/// * `path` - File path to write JSON to
///
/// # Returns
/// * `Ok(())` on success
/// * `Err` if file cannot be created or written
pub fn export_json(
    results: &BenchmarkResults,
    config: &BenchmarkConfig,
    path: &str,
) -> io::Result<()> {
    let json_results = JsonResults {
        total_executions: results.total_executions,
        early_count: results.early_count,
        on_time_count: results.on_time_count,
        late_count: results.late_count,
        missed_count: results.missed_count,

        baseline_delta: results.baseline_delta,

        min_drift: results.min_drift,
        max_drift: results.max_drift,
        avg_drift: results.avg_drift,
        stddev_drift: results.stddev_drift,

        p50_drift: results.p50_drift,
        p95_drift: results.p95_drift,
        p99_drift: results.p99_drift,

        expected_executions: results.expected_executions,
        expected_duration_secs: results.expected_duration_secs,
        actual_duration_secs: results.actual_duration_secs,

        cpu_time_secs: results.cpu_time.as_secs_f64(),
        memory_kb: results.memory_kb,
        context_switches: results.context_switches,

        scheduler_overhead_percent: results.scheduler_overhead_percent,
        coalescing_ratio: results.coalescing_ratio,
        wakeup_count: results.wakeup_count,
        avg_tasks_per_wakeup: results.avg_tasks_per_wakeup,
        avg_task_execution_us: results.avg_task_execution_us,

        top_outliers: results
            .top_outliers
            .iter()
            .map(|(exec_num, drift, category, notes)| OutlierData {
                execution_num: *exec_num,
                drift: *drift,
                category: category.clone(),
                notes: notes.clone(),
            })
            .collect(),
    };

    let export = JsonExport {
        config: config.clone(),
        results: json_results,
    };

    let json_string = serde_json::to_string_pretty(&export)?;
    let mut file = File::create(path)?;
    file.write_all(json_string.as_bytes())?;

    Ok(())
}

/// CSV record for per-execution time-series data
#[derive(Debug, Serialize)]
struct CsvRecord {
    execution_num: usize,
    timer_id: usize,
    ideal_time_us: u64,
    actual_time_us: u64,
    drift_us: i64,
    category: String,
}

/// Export benchmark results to CSV format
///
/// Creates a CSV file with per-execution time-series data suitable for
/// analysis in Excel, R, Python, or Grafana.
///
/// # CSV Columns
/// * `execution_num` - Sequential execution number (1-based)
/// * `timer_id` - Timer identifier
/// * `ideal_time_us` - Expected execution time in microseconds
/// * `actual_time_us` - Actual execution time in microseconds
/// * `drift_us` - Timing drift (actual - ideal) in microseconds
/// * `category` - Execution category (Early, On-Time, Late)
///
/// # Arguments
/// * `results` - The benchmark results to export
/// * `path` - File path to write CSV to
///
/// # Returns
/// * `Ok(())` on success
/// * `Err` if file cannot be created or written
///
/// # Note
/// This function exports aggregated data from first/last executions and outliers.
/// For full per-execution data, the benchmark runner should be modified to
/// pass raw timestamp data.
pub fn export_csv(results: &BenchmarkResults, path: &str) -> io::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;

    // Export first executions
    for (exec_num, drift, category) in &results.first_executions {
        let record = CsvRecord {
            execution_num: *exec_num,
            timer_id: 0,       // Timer ID not available in aggregated results
            ideal_time_us: 0,  // Would need raw timestamps
            actual_time_us: 0, // Would need raw timestamps
            drift_us: *drift,
            category: category.clone(),
        };
        writer.serialize(record)?;
    }

    // Export last executions
    for (exec_num, drift, category) in &results.last_executions {
        let record = CsvRecord {
            execution_num: *exec_num,
            timer_id: 0,
            ideal_time_us: 0,
            actual_time_us: 0,
            drift_us: *drift,
            category: category.clone(),
        };
        writer.serialize(record)?;
    }

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_results() -> BenchmarkResults {
        BenchmarkResults {
            total_executions: 1000,
            early_count: 50,
            on_time_count: 900,
            late_count: 50,
            missed_count: 0,
            baseline_delta: -5,
            min_drift: -50.0,
            max_drift: 50.0,
            avg_drift: 0.0,
            stddev_drift: 10.0,
            p50_drift: 0.0,
            p95_drift: 20.0,
            p99_drift: 40.0,
            expected_executions: 1000,
            expected_duration_secs: 10.0,
            actual_duration_secs: 10.005,
            cpu_time: Duration::from_millis(100),
            memory_kb: 1024,
            context_switches: 50,
            scheduler_overhead_percent: 2.5,
            coalescing_ratio: 0.95,
            wakeup_count: 950,
            avg_tasks_per_wakeup: 1.05,
            avg_task_execution_us: 50.0,
            first_executions: vec![
                (1, -5, "On-Time".to_string()),
                (2, 3, "On-Time".to_string()),
            ],
            last_executions: vec![
                (999, -2, "On-Time".to_string()),
                (1000, 1, "On-Time".to_string()),
            ],
            top_outliers: vec![
                (100, 45, "Late".to_string(), "anomaly".to_string()),
                (500, -40, "Early".to_string(), "warm-up".to_string()),
            ],
        }
    }

    #[test]
    fn test_json_export() {
        let results = create_test_results();
        let config = BenchmarkConfig {
            system: "syncopate".to_string(),
            scenario: Some("steady-state".to_string()),
            duration_secs: 10,
            timers: 1,
            task_period_us: 10000,
        };

        let path = "/tmp/test_benchmark_export.json";
        export_json(&results, &config, path).expect("JSON export should succeed");

        // Verify file exists and is valid JSON
        let contents = std::fs::read_to_string(path).expect("Should read file");
        let _parsed: JsonExport =
            serde_json::from_str(&contents).expect("Should parse as valid JSON");

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_csv_export() {
        let results = create_test_results();
        let path = "/tmp/test_benchmark_export.csv";

        export_csv(&results, path).expect("CSV export should succeed");

        // Verify file exists and has correct structure
        let contents = std::fs::read_to_string(path).expect("Should read file");
        assert!(contents.contains("execution_num"));
        assert!(contents.contains("drift_us"));
        assert!(contents.contains("category"));

        std::fs::remove_file(path).ok();
    }
}
