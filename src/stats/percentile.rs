#[derive(Debug)]
pub struct PercentileSummary {
    pub metric_id: &'static str,
    pub count: usize,
    pub p50: usize,
    pub p90: usize,
    pub p95: usize,
    pub p99: usize,
    pub max: usize,
}

impl PercentileSummary {
    pub fn from_values(metric_id: &'static str, values: &[usize]) -> Self {
        if values.is_empty() {
            return Self {
                metric_id,
                count: 0,
                p50: 0,
                p90: 0,
                p95: 0,
                p99: 0,
                max: 0,
            };
        }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        Self {
            metric_id,
            count: sorted.len(),
            p50: percentile(&sorted, 50.0),
            p90: percentile(&sorted, 90.0),
            p95: percentile(&sorted, 95.0),
            p99: percentile(&sorted, 99.0),
            max: *sorted.last().unwrap_or(&0),
        }
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn percentile(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let len = sorted.len();
    let idx_f = (len.saturating_sub(1) as f64) * p / 100.0;
    let idx = idx_f.round().max(0.0) as usize;
    sorted[idx.min(len - 1)]
}
