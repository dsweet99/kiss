use super::*;
use std::time::Duration;

#[test]
fn format_runtime_ms_line_requires_samples() {
    assert!(format_unit_test_runtime_ms_line(&[]).is_none());
    let line = format_unit_test_runtime_ms_line(&[
        UnitTestTiming {
            language: Language::Python,
            selector: "a".into(),
            duration: Duration::from_millis(12),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "b".into(),
            duration: Duration::from_millis(40),
        },
    ])
    .unwrap();
    assert!(line.starts_with("unit_test_runtime_ms: N=2 "));
    assert!(line.contains("p50="));
    assert!(line.contains("max=40"));
}

#[test]
fn evaluate_runtime_gate_threshold_semantics() {
    let timings = TimingPopulation::Complete(vec![UnitTestTiming {
        language: Language::Python,
        selector: "t::slow".into(),
        duration: Duration::from_millis(1500),
    }]);
    assert!(matches!(
        evaluate_runtime_gate(&timings, 0.0),
        RuntimeGateEval::Disabled
    ));
    match evaluate_runtime_gate(&timings, 1.0) {
        RuntimeGateEval::Failed(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].selector, "t::slow");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(matches!(
        evaluate_runtime_gate(&timings, 2.0),
        RuntimeGateEval::Passed
    ));
    // Equality at threshold fails.
    let exact = TimingPopulation::Complete(vec![UnitTestTiming {
        language: Language::Rust,
        selector: "exact".into(),
        duration: Duration::from_secs(2),
    }]);
    assert!(matches!(
        evaluate_runtime_gate(&exact, 2.0),
        RuntimeGateEval::Failed(_)
    ));
}

#[test]
fn incomplete_population_fail_closed_when_enabled() {
    assert!(matches!(
        evaluate_runtime_gate(&TimingPopulation::Incomplete, 2.0),
        RuntimeGateEval::Incomplete
    ));
}

#[test]
fn runtime_gate_failure_lines_are_sorted_and_labeled() {
    let lines = runtime_gate_failure_lines(
        &[
            RuntimeGateViolation {
                language: Language::Rust,
                selector: "crate::b".into(),
                seconds: 3.0,
            },
            RuntimeGateViolation {
                language: Language::Python,
                selector: "tests/test_x.py::test_y".into(),
                seconds: 2.41,
            },
        ],
        2.0,
    );
    assert_eq!(
        lines[0],
        "GATE_FAILED:max_unit_test_seconds: 2 test(s) at or above 2.00s"
    );
    assert_eq!(
        lines[1],
        "  [python] tests/test_x.py::test_y: 2.41s (limit 2.00s)"
    );
    assert_eq!(lines[2], "  [rust] crate::b: 3.00s (limit 2.00s)");
}

#[test]
fn empty_lang_selection_is_complete_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = collect_current_unit_test_timings(TimingCollectOpts {
        universe: tmp.path(),
        lang_filter: None,
        include: TimingLangInclude {
            python: false,
            rust: false,
        },
        ignore: &[],
    });
    assert_eq!(pop, TimingPopulation::Complete(vec![]));
}

#[test]
fn ignore_prefixes_drop_matching_path_selectors() {
    let timings = vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fast/test_a.py::t".into(),
            duration: Duration::from_millis(10),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/slow/test_b.py::t".into(),
            duration: Duration::from_millis(50),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "crate::mod::tests::t".into(),
            duration: Duration::from_millis(20),
        },
    ];
    let filtered = filter_timings_by_ignore(timings, &["tests/slow".into()]);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].selector, "tests/fast/test_a.py::t");
    assert_eq!(filtered[1].selector, "crate::mod::tests::t");
}
