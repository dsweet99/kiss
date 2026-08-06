use super::*;
use std::time::Instant;

#[cfg(unix)]
use crate::test_runner::capture_stdout::capture_stdout;

#[cfg(unix)]
#[test]
fn finish_paths_print_recap_with_plan_time_and_phase_order() {
    let planned = planned();
    let no_work_options = SelectorRunOptions {
        dry_run: false,
        force_rerun: false,
        metrics: true,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        plan_duration: Duration::from_millis(200),
    };
    let no_work = capture_stdout(|| {
        assert_eq!(run_selectors(&planned, no_work_options).unwrap(), 0);
    });
    assert!(no_work.contains("NO COVERING TESTS"));
    assert!(
        no_work.contains("✓ 0 passed"),
        "no-work must print empty recap: {no_work}"
    );
    assert!(
        no_work.contains("0s max pass"),
        "no-work max pass must be 0s: {no_work}"
    );
    let total_ms = no_work
        .lines()
        .find(|line| line.contains("✓ 0 passed") && line.contains(" total · "))
        .and_then(|line| {
            line.split(" · ")
                .find_map(|part| part.strip_suffix("s total")?.parse::<f64>().ok())
        })
        .expect("recap total seconds");
    assert!(
        total_ms >= 0.20,
        "recap total must include plan_duration 200ms, got {total_ms}s in {no_work}"
    );
    let metrics_idx = no_work.find("phase_plan_ms=").expect("metrics");
    let recap_idx = no_work.find("✓ 0 passed").expect("recap");
    assert!(
        metrics_idx < recap_idx,
        "metrics must print before recap: {no_work}"
    );

    let phase_options = SelectorRunOptions {
        dry_run: false,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        plan_duration: Duration::from_millis(200),
    };
    let python = FakeLanguageModule {
        language: Language::Python,
        population_required: false,
        selective: vec!["tests/test_a.py::test_a".to_string()],
        summary: SelectorExecutionSummary {
            total: 1,
            failed: 1,
            failed_selectors: vec!["tests/test_a.py::test_a".to_string()],
            max_passing_run_duration: Duration::ZERO,
            exit_code: 1,
            ..SelectorExecutionSummary::default()
        },
    };
    let rust = FakeLanguageModule {
        language: Language::Rust,
        population_required: false,
        selective: vec![
            "tests::rust_pass".to_string(),
            "tests::rust_fail".to_string(),
        ],
        summary: SelectorExecutionSummary {
            total: 2,
            failed: 1,
            failed_selectors: vec!["tests::rust_fail".to_string()],
            max_passing_run_duration: Duration::from_millis(50),
            exit_code: 1,
            ..SelectorExecutionSummary::default()
        },
    };
    let modules: [(&dyn LanguageTestModule, ExecutionPhase); 2] = [
        (
            &python,
            ExecutionPhase::Selective(vec!["tests/test_a.py::test_a".to_string()]),
        ),
        (
            &rust,
            ExecutionPhase::Selective(vec![
                "tests::rust_pass".to_string(),
                "tests::rust_fail".to_string(),
            ]),
        ),
    ];
    let out = capture_stdout(|| {
        let code =
            run_selected_phases(&planned, &phase_options, Instant::now(), &modules).unwrap();
        assert_eq!(code, 1);
    });
    assert!(
        out.contains("✗ 1 passed · 2 failed"),
        "expected failure recap: {out}"
    );
    let py_fail = out
        .find("FAILED tests/test_a.py::test_a")
        .expect("python failure");
    let rs_fail = out.find("FAILED tests::rust_fail").expect("rust failure");
    assert!(py_fail < rs_fail, "python failures before rust: {out}");
    assert!(out.contains("0.05s max pass"), "fresh rust max pass: {out}");
}

#[cfg(unix)]
#[test]
fn dry_run_does_not_print_final_recap() {
    let mut with_work = planned();
    with_work.py_sel = vec!["tests/test_app.py::test_ok".to_string()];
    let out = capture_stdout(|| {
        assert_eq!(
            run_selectors(
                &with_work,
                SelectorRunOptions {
                    dry_run: true,
                    force_rerun: false,
                    metrics: false,
                    jobs: 1,
                    extra: &[],
                    python_extra: &[],
                    plan_duration: Duration::from_millis(10),
                },
            )
            .unwrap(),
            0
        );
    });
    assert!(
        !out.contains(" passed · "),
        "dry-run must not print recap: {out}"
    );
    assert!(
        !out.contains("FAILED "),
        "dry-run must not print FAILED recap lines: {out}"
    );
}
