use std::cell::Cell;
use std::time::{Duration, Instant};

use super::report::{EffectiveStatus, TargetPlanPreview, TargetReport};
use super::scope::ExecutionPlan;
use crate::test_runner::duration::format_test_duration;

thread_local! {
    static RUN_STARTED: Cell<Option<Instant>> = const { Cell::new(None) };
    static TOTAL_OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
}

pub(crate) struct KissTestRunClock {
    clear_on_drop: bool,
}

impl KissTestRunClock {
    pub(crate) fn start() -> Self {
        RUN_STARTED.set(Some(Instant::now()));
        Self {
            clear_on_drop: true,
        }
    }

    pub(crate) fn ensure() -> Self {
        let missing = RUN_STARTED.with(|slot| slot.get().is_none());
        if missing {
            RUN_STARTED.set(Some(Instant::now()));
        }
        Self {
            clear_on_drop: missing,
        }
    }
}

impl Drop for KissTestRunClock {
    fn drop(&mut self) {
        if self.clear_on_drop {
            RUN_STARTED.set(None);
        }
    }
}

fn kiss_test_total() -> Duration {
    if let Some(fixed) = TOTAL_OVERRIDE.get() {
        return fixed;
    }
    RUN_STARTED.with(|slot| {
        slot.get()
            .map(|started| started.elapsed())
            .unwrap_or(Duration::ZERO)
    })
}

pub(crate) fn render_plan_preview(preview: &TargetPlanPreview) {
    crate::test_runner::emit_test_progress(&format!(
        "kiss test: plan complete={} deferred={}",
        preview.membership_complete, preview.deferred
    ));
    render_execution_plan(&preview.plan);
}

pub(crate) fn render_preview_members(preview: &TargetPlanPreview) {
    for selector in &preview.scope.selectors {
        crate::test_runner::emit_test_progress(selector);
    }
}

pub(crate) fn official_report_text(report: &TargetReport) -> String {
    let mut out = String::new();
    for row in &report.rows {
        if row.effective == EffectiveStatus::Pass {
            continue;
        }
        out.push_str(official_label(row.effective));
        out.push(' ');
        out.push_str(report.labels.get(&row.selector).unwrap_or(&row.selector));
        out.push('\n');
    }
    out.push_str(&official_summary_text(report));
    out
}

pub(crate) fn official_summary_text(report: &TargetReport) -> String {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    for row in &report.rows {
        match row.effective {
            EffectiveStatus::Pass => passed += 1,
            EffectiveStatus::Fail => failed += 1,
            EffectiveStatus::Timeout => timed_out += 1,
        }
    }
    let mark = if failed + timed_out == 0 {
        "✓"
    } else {
        "✗"
    };
    let durations = individual_durations_ns(report);
    let median = format_optional_duration(median_ns(&durations));
    let max = format_optional_duration(durations.last().copied());
    let total = format_test_duration(kiss_test_total());
    let mut out = format!(
        "kiss test: report members={} exit={}\n",
        report.rows.len(),
        report.exit_code
    );
    out.push_str(&official_gate_text(report));
    out.push_str(&format!(
        "{mark} {passed} passed · {failed} failed · {timed_out} timed out · {median} median · {max} max · {total} total\n"
    ));
    out
}

pub(crate) fn official_gate_text(report: &TargetReport) -> String {
    let mut out = String::new();
    let time_gates: Vec<_> = report
        .gates
        .iter()
        .filter(|gate| gate.kind == "max_unit_test_seconds")
        .collect();
    let count_gates: Vec<_> = report
        .gates
        .iter()
        .filter(|gate| gate.kind == "max_num_tests")
        .collect();
    let orphan_gates: Vec<_> = report
        .gates
        .iter()
        .filter(|gate| gate.kind == "orphan")
        .collect();
    if time_gates.is_empty() && count_gates.is_empty() && orphan_gates.is_empty() {
        out.push_str("NO VIOLATIONS\n");
        return out;
    }
    if !time_gates.is_empty() {
        out.push_str(&format!(
            "VIOLATION:max_unit_test_seconds: {} test(s) exceeded path-pattern time limits\n",
            time_gates.len()
        ));
    }
    for gate in count_gates {
        out.push_str("VIOLATION:max_num_tests: ");
        out.push_str(&gate.detail);
        out.push('\n');
    }
    for gate in orphan_gates {
        out.push_str("VIOLATION:orphan:");
        out.push_str(&gate.detail);
        out.push('\n');
    }
    out.push_str(kiss::cli_output::VIOLATIONS_FIX_HINT);
    out.push('\n');
    out
}

pub(crate) fn render_official_report(report: &TargetReport) {
    for line in official_report_text(report).lines() {
        crate::test_runner::emit_test_progress(line);
    }
}

fn individual_durations_ns(report: &TargetReport) -> Vec<u64> {
    let mut values: Vec<u64> = report
        .rows
        .iter()
        .filter_map(|row| row.duration_ns)
        .collect();
    values.sort_unstable();
    values
}

fn median_ns(sorted: &[u64]) -> Option<u64> {
    let count = sorted.len();
    if count == 0 {
        return None;
    }
    if count % 2 == 1 {
        return Some(sorted[count / 2]);
    }
    let left = u128::from(sorted[count / 2 - 1]);
    let right = u128::from(sorted[count / 2]);
    u64::try_from((left + right) / 2).ok()
}

fn format_optional_duration(duration_ns: Option<u64>) -> String {
    match duration_ns {
        Some(duration_ns) => format_test_duration(Duration::from_nanos(duration_ns)),
        None => "n/a".to_string(),
    }
}

fn official_label(status: EffectiveStatus) -> &'static str {
    match status {
        EffectiveStatus::Pass => "PASS",
        EffectiveStatus::Fail => "FAIL",
        EffectiveStatus::Timeout => "TIMEOUT",
    }
}

fn render_execution_plan(plan: &ExecutionPlan) {
    let union = plan.known_execution_union();
    crate::test_runner::emit_test_progress(&format!(
        "kiss test: plan execute={} population={} graph={}",
        union.len(),
        plan.population_repair,
        plan.graph_repair
    ));
}

#[cfg(test)]
mod official_text_tests {
    use super::*;
    use std::time::Duration;

    use crate::test_runner::target_request::report::{
        ReportEvidenceStamp, ReportGate, ReportSnapshot, SelectorRow,
    };
    use crate::test_runner::target_request::scope::ReportScope;
    use crate::test_runner::target_request::slice::TargetSliceStamp;

    fn report(gates: Vec<ReportGate>) -> TargetReport {
        TargetReport {
            scope: ReportScope::from_membership(
                Vec::new(),
                vec!["tests/a.py::test_a".into()],
                true,
            ),
            rows: vec![SelectorRow {
                language: kiss::Language::Python,
                selector: "tests/a.py::test_a".into(),
                raw: "passed".into(),
                effective: EffectiveStatus::Pass,
                duration_ns: None,
                provenance: "witness".into(),
            }],
            stamp: TargetSliceStamp {
                digest: "d".into(),
                complete: true,
                index_schema: "target-slice-v1".into(),
            },
            exit_code: 0,
            evidence: ReportEvidenceStamp { digest: "e".into() },
            gates,
            graph_generation: None,
            snapshot: ReportSnapshot::default(),
            labels: Default::default(),
        }
    }

    #[test]
    fn official_text_prints_no_violations_when_gates_empty() {
        let text = official_report_text(&report(Vec::new()));
        assert!(text.contains("NO VIOLATIONS"), "{text}");
        assert!(!text.contains("VIOLATION:"), "{text}");
        assert!(
            !text.lines().any(|line| line.starts_with("PASS ")),
            "cached PASS has no line: {text}"
        );
        assert!(text.contains("1 passed"), "{text}");
        assert!(text.contains("n/a median · n/a max ·"), "{text}");
    }

    #[test]
    fn official_summary_reports_median_max_and_total() {
        let mut sample = report(Vec::new());
        sample.rows = vec![
            timed("tests/a.py::test_a", EffectiveStatus::Pass, 100_000_000),
            timed("tests/a.py::test_b", EffectiveStatus::Fail, 300_000_000),
            timed("tests/a.py::test_c", EffectiveStatus::Timeout, 500_000_000),
            timed_missing("tests/a.py::test_d"),
        ];
        super::TOTAL_OVERRIDE.set(Some(Duration::from_millis(1250)));
        let text = official_summary_text(&sample);
        super::TOTAL_OVERRIDE.set(None);
        assert_eq!(
            text,
            "kiss test: report members=4 exit=0\n\
NO VIOLATIONS\n\
✗ 2 passed · 1 failed · 1 timed out · 0.30s median · 0.50s max · 1.25s total\n"
        );
        assert_eq!(
            text.lines().next_back().unwrap(),
            "✗ 2 passed · 1 failed · 1 timed out · 0.30s median · 0.50s max · 1.25s total"
        );
        assert_eq!(super::median_ns(&[100, 300]), Some(200));
        assert_eq!(super::median_ns(&[]), None);
    }

    fn timed(selector: &str, effective: EffectiveStatus, duration_ns: u64) -> SelectorRow {
        SelectorRow {
            language: kiss::Language::Python,
            selector: selector.into(),
            raw: "status".into(),
            effective,
            duration_ns: Some(duration_ns),
            provenance: "witness".into(),
        }
    }

    fn timed_missing(selector: &str) -> SelectorRow {
        SelectorRow {
            duration_ns: None,
            ..timed(selector, EffectiveStatus::Pass, 0)
        }
    }

    #[test]
    fn official_text_lists_cached_fail_and_timeout_only() {
        let mut cached = report(Vec::new());
        cached.rows.push(SelectorRow {
            language: kiss::Language::Python,
            selector: "tests/a.py::test_bad".into(),
            raw: "failed".into(),
            effective: EffectiveStatus::Fail,
            duration_ns: None,
            provenance: "witness".into(),
        });
        cached.rows.push(SelectorRow {
            language: kiss::Language::Rust,
            selector: "src/lib.rs::test_slow".into(),
            raw: "timeout".into(),
            effective: EffectiveStatus::Timeout,
            duration_ns: None,
            provenance: "witness".into(),
        });
        let text = official_report_text(&cached);
        assert!(
            !text.lines().any(|line| line.starts_with("PASS ")),
            "{text}"
        );
        assert!(text.contains("FAIL tests/a.py::test_bad"), "{text}");
        assert!(text.contains("TIMEOUT src/lib.rs::test_slow"), "{text}");
        assert!(text.contains("1 passed · 1 failed · 1 timed out"), "{text}");
    }

    #[test]
    fn official_text_ignores_unknown_gate_kind() {
        let text = official_report_text(&report(vec![ReportGate {
            kind: "unknown".into(),
            detail: "foo.py".into(),
        }]));
        assert!(!text.contains("VIOLATION:unknown"), "{text}");
        assert!(text.contains("NO VIOLATIONS"), "{text}");
    }

    #[test]
    fn official_text_prints_time_gates() {
        let text = official_report_text(&report(vec![ReportGate {
            kind: "max_unit_test_seconds".into(),
            detail: "tests/a.py::test_a".into(),
        }]));
        assert!(
            text.contains(
                "VIOLATION:max_unit_test_seconds: 1 test(s) exceeded path-pattern time limits"
            ),
            "{text}"
        );
        assert!(!text.contains("NO VIOLATIONS"), "{text}");
    }

    #[test]
    fn official_text_prints_max_num_tests_gate() {
        let text = official_report_text(&report(vec![ReportGate {
            kind: "max_num_tests".into(),
            detail: "3 test(s) exceeds max_num_tests=2".into(),
        }]));
        assert!(
            text.contains("VIOLATION:max_num_tests: 3 test(s) exceeds max_num_tests=2"),
            "{text}"
        );
        assert!(!text.contains("NO VIOLATIONS"), "{text}");
    }

    #[test]
    fn official_text_prints_orphan_gate() {
        let text = official_report_text(&report(vec![ReportGate {
            kind: "orphan".into(),
            detail: "utils.py:helper".into(),
        }]));
        assert!(text.contains("VIOLATION:orphan:utils.py:helper"), "{text}");
        assert!(!text.contains("NO VIOLATIONS"), "{text}");
    }
}
