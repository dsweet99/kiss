use super::report::{EffectiveStatus, TargetPlanPreview, TargetReport};
use super::scope::ExecutionPlan;

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
    let mut out = format!("{mark} {passed} passed · {failed} failed · {timed_out} timed out\n");
    out.push_str(&format!(
        "kiss test: report members={} exit={}\n",
        report.rows.len(),
        report.exit_code
    ));
    out.push_str(&official_gate_text(report));
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
                language: "python".into(),
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
    }

    #[test]
    fn official_text_lists_cached_fail_and_timeout_only() {
        let mut cached = report(Vec::new());
        cached.rows.push(SelectorRow {
            language: "python".into(),
            selector: "tests/a.py::test_bad".into(),
            raw: "failed".into(),
            effective: EffectiveStatus::Fail,
            duration_ns: None,
            provenance: "witness".into(),
        });
        cached.rows.push(SelectorRow {
            language: "rust".into(),
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
