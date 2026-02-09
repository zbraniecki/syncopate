//! ASCII visualization functions for benchmark data

const BAR_CHARS: [char; 5] = [' ', '░', '▒', '▓', '█'];

/// Renders an ASCII histogram showing the distribution of values
///
/// # Arguments
/// * `values` - Slice of f64 values to plot
/// * `title` - Title for the histogram
/// * `unit` - Unit label (e.g., "μs", "ms")
///
/// # Returns
/// Multi-line string with the histogram visualization
pub fn render_histogram(values: &[f64], title: &str, unit: &str) -> String {
    if values.is_empty() {
        return format!("No data to display for: {}", title);
    }

    let mut output = String::new();

    // Header
    output.push_str("\n╔═══════════════════════════════════════════════════════════════════╗\n");
    output.push_str(&format!("║ {:<65} ║\n", title));
    output.push_str("╠═══════════════════════════════════════════════════════════════════╣\n");

    // Find min/max
    let min_val = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_val = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if (max_val - min_val).abs() < f64::EPSILON {
        output.push_str(&format!(
            "║ All values equal: {:.2} {}                                      ║\n",
            min_val, unit
        ));
        output.push_str("╚═══════════════════════════════════════════════════════════════════╝\n");
        return output;
    }

    // Create buckets
    const NUM_BUCKETS: usize = 15;
    let mut buckets = [0usize; NUM_BUCKETS];
    let bucket_width = (max_val - min_val) / NUM_BUCKETS as f64;

    // Fill buckets
    for &val in values {
        let bucket_idx = ((val - min_val) / bucket_width) as usize;
        let bucket_idx = bucket_idx.min(NUM_BUCKETS - 1);
        buckets[bucket_idx] += 1;
    }

    // Find max bucket count for scaling
    let max_count = *buckets.iter().max().unwrap();

    // Render each bucket
    for (i, &count) in buckets.iter().enumerate() {
        let bucket_start = min_val + (i as f64 * bucket_width);
        let bucket_end = bucket_start + bucket_width;

        // Calculate bar length (max 40 chars)
        let bar_length = if max_count > 0 {
            (count as f64 / max_count as f64 * 40.0) as usize
        } else {
            0
        };

        // Create bar using block characters
        let full_blocks = bar_length / 4;
        let remainder = bar_length % 4;
        let mut bar = String::new();

        for _ in 0..full_blocks {
            bar.push(BAR_CHARS[4]);
        }
        if remainder > 0 && full_blocks < 10 {
            bar.push(BAR_CHARS[remainder]);
        }

        // Calculate percentage
        let percentage = (count as f64 / values.len() as f64) * 100.0;

        // Format bucket range
        let range_str = format!("[{:>7.1},{:>7.1})", bucket_start, bucket_end);

        output.push_str(&format!(
            "║ {:17}{:<40} {:>5.1}% ({:>4}) ║\n",
            range_str, bar, percentage, count
        ));
    }

    output.push_str("║                                                                   ║\n");
    output.push_str(&format!(
        "║ Range: {:.2} to {:.2} {}                              ║\n",
        min_val, max_val, unit
    ));
    output.push_str(&format!(
        "║ Total samples: {}                                               ║\n",
        values.len()
    ));
    output.push_str("╚═══════════════════════════════════════════════════════════════════╝\n");

    output
}

/// Renders an ASCII cumulative distribution function (CDF) plot
///
/// # Arguments
/// * `values` - Slice of f64 values to plot
/// * `title` - Title for the CDF
/// * `unit` - Unit label (e.g., "μs", "ms")
///
/// # Returns
/// Multi-line string with the CDF visualization
pub fn render_cdf(values: &[f64], title: &str, unit: &str) -> String {
    if values.is_empty() {
        return format!("No data to display for: {}", title);
    }

    let mut output = String::new();

    // Header
    output.push_str("\n╔═══════════════════════════════════════════════════════════════════╗\n");
    output.push_str(&format!("║ {:<65} ║\n", title));
    output.push_str("╠═══════════════════════════════════════════════════════════════════╣\n");

    // Sort values for CDF
    let mut sorted_values: Vec<f64> = values.to_vec();
    sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min_val = sorted_values[0];
    let max_val = sorted_values[sorted_values.len() - 1];
    let value_range = max_val - min_val;

    if value_range.abs() < f64::EPSILON {
        output.push_str(&format!(
            "║ All values equal: {:.2} {}                                      ║\n",
            min_val, unit
        ));
        output.push_str("╚═══════════════════════════════════════════════════════════════════╝\n");
        return output;
    }

    // Calculate key percentiles
    let percentiles = [0.25, 0.50, 0.75, 0.90, 0.99];
    let mut percentile_values = Vec::new();
    for &p in &percentiles {
        let idx = ((sorted_values.len() as f64 - 1.0) * p) as usize;
        percentile_values.push((p * 100.0, sorted_values[idx]));
    }

    // Create plot grid
    const HEIGHT: usize = 20;
    const WIDTH: usize = 50;
    let mut grid = vec![vec![' '; WIDTH]; HEIGHT];

    // Plot CDF curve
    for (x, row) in grid.iter_mut().enumerate().take(WIDTH) {
        let percentile = x as f64 / WIDTH as f64;
        let value_idx = (percentile * (sorted_values.len() - 1) as f64) as usize;
        let value = sorted_values[value_idx.min(sorted_values.len() - 1)];

        // Map value to y coordinate (inverted for display)
        let normalized = (value - min_val) / value_range;
        let y = HEIGHT - 1 - (normalized * (HEIGHT - 1) as f64) as usize;

        if y < HEIGHT {
            row[y] = '●';
        }
    }

    // Draw the plot with axis
    output.push_str(&format!("║ 100%┤{}║\n", grid[0].iter().collect::<String>()));

    for (y, row) in grid.iter().enumerate().take(HEIGHT - 1).skip(1) {
        if y == HEIGHT / 4 {
            output.push_str(&format!("║  75%┤{}║\n", row.iter().collect::<String>()));
        } else if y == HEIGHT / 2 {
            output.push_str(&format!("║  50%┤{}║\n", row.iter().collect::<String>()));
        } else if y == 3 * HEIGHT / 4 {
            output.push_str(&format!("║  25%┤{}║\n", row.iter().collect::<String>()));
        } else {
            output.push_str(&format!("║     ┤{}║\n", row.iter().collect::<String>()));
        }
    }

    output.push_str(&format!(
        "║   0%┤{}║\n",
        grid[HEIGHT - 1].iter().collect::<String>()
    ));
    output.push_str(&format!("║     └{}┘║\n", "─".repeat(WIDTH)));
    output.push_str(&format!(
        "║      {:<7.1}{:>40.1} {} ║\n",
        min_val, max_val, unit
    ));

    // Display percentile table
    output.push_str("╠═══════════════════════════════════════════════════════════════════╣\n");
    output.push_str("║ Key Percentiles:                                                  ║\n");
    for (p, val) in percentile_values {
        output.push_str(&format!(
            "║   P{:<4.0} {:<10.2} {}                                         ║\n",
            p, val, unit
        ));
    }

    output.push_str("╚═══════════════════════════════════════════════════════════════════╝\n");

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_basic() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let result = render_histogram(&values, "Test Histogram", "μs");
        assert!(result.contains("Test Histogram"));
        assert!(result.contains("μs"));
    }

    #[test]
    fn test_histogram_empty() {
        let values: Vec<f64> = vec![];
        let result = render_histogram(&values, "Empty", "μs");
        assert!(result.contains("No data"));
    }

    #[test]
    fn test_cdf_basic() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = render_cdf(&values, "Test CDF", "μs");
        assert!(result.contains("Test CDF"));
        assert!(result.contains("P25"));
        assert!(result.contains("P50"));
    }
}
