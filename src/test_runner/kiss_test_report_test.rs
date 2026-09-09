use super::*;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::{RunTestOnceOutcome, WatchCoverageResult};

fn dry_args() -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        invocation: TestInvocation::Targets(vec!["tests/a.py".into()]),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

#[test]
fn shared_report_is_the_captured_transcript_not_a_rebuild() {
    let args = dry_args();
    let report = run_kiss_test_report(
        args,
        |_a| {
            crate::test_runner::emit_test_progress("kiss test: Planning ...");
            crate::test_runner::emit_test_progress("PASS: tests/a.py::test_a (0.01s)");
            crate::test_runner::emit_test_progress(
                "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            RunTestOnceOutcome::Code(0)
        },
        |_a| WatchCoverageResult::ok(0),
    );
    let out = report.output.unwrap_or_default();
    assert!(out.contains("kiss test: Planning ..."), "{out}");
    assert!(out.contains("PASS: tests/a.py::test_a"), "{out}");
    assert!(out.contains("✓ 1 passed"), "{out}");
    assert!(!out.contains("PASS (cached): 1 selectors"), "{out}");
    assert_eq!(report.exit_code, 0);
    assert!(!report.interrupted);
}

#[test]
fn shared_report_skips_coverage_on_test_failure() {
    let mut args = dry_args();
    args.dry_run = false;
    let covs = std::sync::atomic::AtomicUsize::new(0);
    let report = run_kiss_test_report(
        args,
        |_a| RunTestOnceOutcome::Code(2),
        |_a| {
            covs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            WatchCoverageResult::ok(0)
        },
    );
    assert_eq!(report.exit_code, 2);
    assert_eq!(covs.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn shared_report_puts_recap_after_coverage_violations() {
    let mut args = dry_args();
    args.dry_run = false;
    let report = run_kiss_test_report(
        args,
        |_a| {
            crate::test_runner::final_summary::print_final_test_summary(
                &crate::test_runner::final_summary::FinalTestSummary {
                    passed: 1,
                    failed: 0,
                    ..crate::test_runner::final_summary::FinalTestSummary::default()
                },
                std::time::Duration::from_millis(10),
            );
            RunTestOnceOutcome::Code(0)
        },
        |_a| {
            crate::test_runner::final_summary::note_violation_kind("test_coverage", 2);
            crate::test_runner::emit_test_progress(
                "VIOLATION:test_coverage:foo.py:1:foo: 0% covered (0/4). Need 3 more lines to reach 75%.",
            );
            WatchCoverageResult::failed(1, "coverage gate failed")
        },
    );
    let out = report.output.unwrap_or_default();
    let viol_at = out.find("VIOLATION:test_coverage:").expect("violation");
    let recap_at = out.find("✓ 1 passed").expect("recap");
    assert!(viol_at < recap_at, "recap after violations:\n{out}");
    assert!(
        out.contains("· 2 test_coverage"),
        "recap must summarize violation types:\n{out}"
    );
    assert_eq!(report.exit_code, 1);
}

