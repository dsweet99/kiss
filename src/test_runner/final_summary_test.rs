use super::*;
use crate::test_runner::duration::format_test_duration;
use crate::test_runner::runners::SelectorExecutionSummary;
use std::time::Duration;

fn summary_with(
    total: usize,
    failed: usize,
    failed_selectors: &[&str],
    max_pass: Duration,
) -> SelectorExecutionSummary {
    SelectorExecutionSummary {
        total,
        failed,
        failed_selectors: failed_selectors.iter().map(|s| (*s).to_string()).collect(),
        max_passing_run_duration: max_pass,
        ..SelectorExecutionSummary::default()
    }
}

#[test]
fn format_all_pass_and_zero_max_pass() {
    let summary =
        FinalTestSummary::absorb(&[&summary_with(148, 0, &[], Duration::from_millis(1370))]);
    assert_eq!(
        format_final_test_summary(&summary, Duration::from_secs_f64(12.84), false),
        "✓ 148 passed · 0 failed · 0 timed out · 12.84s total · 1.37s max pass"
    );

    let cached = FinalTestSummary::absorb(&[&summary_with(148, 0, &[], Duration::ZERO)]);
    assert_eq!(
        format_final_test_summary(&cached, Duration::from_secs_f64(0.42), false),
        "✓ 148 passed · 0 failed · 0 timed out · 0.42s total · 0s max pass"
    );
}

#[test]
fn format_failures_preserve_order_and_full_selectors() {
    let python = summary_with(
        2,
        1,
        &["tests/test_api.py::TestGateway::test_rejects_expired_token"],
        Duration::from_millis(100),
    );
    let rust = summary_with(
        2,
        2,
        &[
            "tests/test_models.py::test_round_trip[parametrize-case-2]",
            "src/rslip/cache_test.rs::missing_file_is_not_reusable",
        ],
        Duration::from_millis(200),
    );
    let summary = FinalTestSummary::absorb(&[&python, &rust]);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 3);
    let text = format_final_test_summary(&summary, Duration::from_secs_f64(12.84), false);
    assert_eq!(
        text,
        "✗ 1 passed · 3 failed · 0 timed out · 12.84s total · 0.20s max pass\n\
FAIL tests/test_api.py::TestGateway::test_rejects_expired_token\n\
FAIL tests/test_models.py::test_round_trip[parametrize-case-2]\n\
FAIL src/rslip/cache_test.rs::missing_file_is_not_reusable"
    );
}

#[test]
fn format_zero_pass_and_no_test_aggregates() {
    let empty = FinalTestSummary::absorb(&[]);
    assert_eq!(
        format_final_test_summary(&empty, Duration::from_secs_f64(0.03), false),
        "✓ 0 passed · 0 failed · 0 timed out · 0.03s total · 0s max pass"
    );
    assert_eq!(format_max_pass_duration(Duration::ZERO), "0s");
    assert_eq!(format_max_pass_duration(Duration::from_millis(10)), "0.01s");
    assert_eq!(format_test_duration(Duration::ZERO), "0.00s");
}

#[test]
fn format_color_only_icons_and_failed_token() {
    let summary = FinalTestSummary {
        passed: 1,
        failed: 1,
        failed_selectors: vec!["tests::boom".to_string()],
        timed_out_selectors: Vec::new(),
        max_passing_run_duration: Duration::from_millis(50),
    };
    let colored = format_final_test_summary(&summary, Duration::from_secs(1), true);
    assert!(colored.starts_with("\x1b[31m✗\x1b[0m "));
    assert!(colored.contains("\x1b[31mFAIL\x1b[0m tests::boom"));
    assert!(!colored.contains("\x1b[31m1 passed"));
    let plain = format_final_test_summary(&summary, Duration::from_secs(1), false);
    assert!(!plain.contains('\x1b'));
    let pass = FinalTestSummary {
        passed: 2,
        failed: 0,
        failed_selectors: Vec::new(),
        timed_out_selectors: Vec::new(),
        max_passing_run_duration: Duration::from_millis(10),
    };
    assert!(
        format_final_test_summary(&pass, Duration::from_secs(1), true)
            .starts_with("\x1b[32m✓\x1b[0m ")
    );
}

#[test]
fn format_timeout_count_on_recap_line() {
    let summary = FinalTestSummary {
        passed: 1,
        failed: 1,
        failed_selectors: Vec::new(),
        timed_out_selectors: vec!["slow::test".to_string()],
        max_passing_run_duration: Duration::ZERO,
    };
    let text = format_final_test_summary(&summary, Duration::from_secs(1), false);
    assert_eq!(
        text,
        "✗ 1 passed · 0 failed · 1 timed out · 1.00s total · 0s max pass\n\
TIMEOUT slow::test"
    );
}

#[test]
fn coverage_skip_message_only_when_failures_or_timeouts() {
    let pass = FinalTestSummary {
        passed: 2,
        failed: 0,
        ..FinalTestSummary::default()
    };
    assert_eq!(coverage_skip_for_failures_message(&pass), None);

    let failed = FinalTestSummary {
        passed: 1,
        failed: 1,
        failed_selectors: vec!["boom".to_string()],
        ..FinalTestSummary::default()
    };
    assert_eq!(
        coverage_skip_for_failures_message(&failed),
        Some("kiss test: skipping coverage because tests failed or timed out")
    );

    let timed_out = FinalTestSummary {
        passed: 1,
        failed: 1,
        timed_out_selectors: vec!["slow".to_string()],
        ..FinalTestSummary::default()
    };
    assert_eq!(
        coverage_skip_for_failures_message(&timed_out),
        Some("kiss test: skipping coverage because tests failed or timed out")
    );
}

#[cfg(unix)]
#[test]
fn print_emits_skip_message_immediately_before_summary() {
    let failed = FinalTestSummary {
        passed: 1,
        failed: 1,
        failed_selectors: vec!["boom".to_string()],
        ..FinalTestSummary::default()
    };
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        print_final_test_summary(&failed, Duration::from_secs(1));
    });
    let skip = "kiss test: skipping coverage because tests failed or timed out";
    let skip_at = out.find(skip).expect(skip);
    let recap_at = out.find("✗ 1 passed").expect("recap");
    assert!(
        skip_at < recap_at,
        "skip message must precede summary:\n{out}"
    );
    assert!(
        !out[..skip_at].contains("passed"),
        "no recap before skip:\n{out}"
    );

    let pass = FinalTestSummary {
        passed: 2,
        failed: 0,
        ..FinalTestSummary::default()
    };
    let pass_out = crate::test_runner::capture_stdout::capture_stdout(|| {
        print_final_test_summary(&pass, Duration::from_secs(1));
    });
    assert!(
        !pass_out.contains(skip),
        "all-pass must still check coverage:\n{pass_out}"
    );
}
