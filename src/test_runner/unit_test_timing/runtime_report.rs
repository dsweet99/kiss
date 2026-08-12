//! Grouped `unit_test_runtime_sec` table for `kiss stats`.

use std::time::Duration;

use kiss::stats::PercentileSummary;

use super::UnitTestTiming;

fn format_secs_from_millis(ms: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    {
        format!("{:.2}", ms as f64 / 1000.0)
    }
}

fn duration_ms(duration: Duration) -> usize {
    #[allow(clippy::cast_possible_truncation)]
    {
        duration.as_millis() as usize
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UnitTestRuntimeGroupRow {
    pub(crate) pattern: String,
    pub(crate) limit_seconds: f64,
    pub(crate) sample_count: usize,
    pub(crate) p50_ms: Option<usize>,
    pub(crate) p90_ms: Option<usize>,
    pub(crate) p95_ms: Option<usize>,
    pub(crate) p99_ms: Option<usize>,
    pub(crate) max_ms: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UnitTestRuntimeGroupedReport {
    pub(crate) codebase_tests: Option<usize>,
    pub(crate) rows: Vec<UnitTestRuntimeGroupRow>,
}

/// Partition cached timings by the ordered first-match `max_unit_test_seconds` rules.
///
/// Returns `None` only when there are no configured rules. Otherwise every rule gets a
/// row, including groups with zero available samples.
pub(crate) fn build_unit_test_runtime_grouped_report(
    timings: &[UnitTestTiming],
    rules: &[(String, f64)],
    codebase_tests: Option<usize>,
) -> Option<UnitTestRuntimeGroupedReport> {
    if rules.is_empty() {
        return None;
    }
    let mut buckets: Vec<Vec<usize>> = rules.iter().map(|_| Vec::new()).collect();
    for timing in timings {
        if let Some(matched) =
            kiss::gate_config::matched_rule_for_selector(rules, &timing.selector)
        {
            buckets[matched.index].push(duration_ms(timing.duration));
        }
    }
    let rows = rules
        .iter()
        .zip(buckets.iter())
        .map(|((pattern, limit), values_ms)| row_from_bucket(pattern, *limit, values_ms))
        .collect();
    Some(UnitTestRuntimeGroupedReport {
        codebase_tests,
        rows,
    })
}

fn row_from_bucket(
    pattern: &str,
    limit_seconds: f64,
    values_ms: &[usize],
) -> UnitTestRuntimeGroupRow {
    if values_ms.is_empty() {
        return UnitTestRuntimeGroupRow {
            pattern: pattern.to_string(),
            limit_seconds,
            sample_count: 0,
            p50_ms: None,
            p90_ms: None,
            p95_ms: None,
            p99_ms: None,
            max_ms: None,
        };
    }
    let summary = PercentileSummary::from_values("unit_test_runtime_sec", values_ms);
    UnitTestRuntimeGroupRow {
        pattern: pattern.to_string(),
        limit_seconds,
        sample_count: summary.count,
        p50_ms: Some(summary.p50),
        p90_ms: Some(summary.p90),
        p95_ms: Some(summary.p95),
        p99_ms: Some(summary.p99),
        max_ms: Some(summary.max),
    }
}

fn format_optional_secs(ms: Option<usize>) -> String {
    match ms {
        Some(v) => format_secs_from_millis(v),
        None => "-".to_string(),
    }
}

pub(crate) fn format_unit_test_runtime_grouped_report(
    report: &UnitTestRuntimeGroupedReport,
) -> String {
    let mut out = String::from(
        "unit_test_runtime_sec: (coverage cache; may not reflect full test set)",
    );
    if let Some(total) = report.codebase_tests {
        out.push_str(&format!(" codebase_tests={total}"));
    }
    out.push('\n');
    out.push_str("pattern\tlimit_s\tN\tp50\tp90\tp95\tp99\tmax");
    for row in &report.rows {
        out.push('\n');
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.pattern,
            format_limit_seconds(row.limit_seconds),
            row.sample_count,
            format_optional_secs(row.p50_ms),
            format_optional_secs(row.p90_ms),
            format_optional_secs(row.p95_ms),
            format_optional_secs(row.p99_ms),
            format_optional_secs(row.max_ms),
        ));
    }
    out
}

fn format_limit_seconds(secs: f64) -> String {
    if secs.fract() == 0.0 && secs.abs() < 1e15 {
        format!("{secs:.0}")
    } else {
        format!("{secs}")
    }
}

#[cfg(test)]
#[path = "runtime_report_test.rs"]
mod tests;
