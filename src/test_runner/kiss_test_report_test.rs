use super::*;
use crate::test_runner::RunTestOnceOutcome;

fn dry_args() -> RunTestCmdArgs<'static> {
    crate::test_runner::test_mode_fixtures::python_dry_run_args(vec!["tests/a.py".into()])
}

#[test]
fn shared_report_is_the_captured_transcript_not_a_rebuild() {
    let args = dry_args();
    let report = run_kiss_test_report(args, |_a| {
        crate::test_runner::emit_test_progress("kiss test: Planning ...");
        crate::test_runner::emit_test_progress("PASS: tests/a.py::test_a (0.01s)");
        crate::test_runner::emit_test_progress(
            "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
        );
        RunTestOnceOutcome::Code(0)
    });
    let out = report.output.unwrap_or_default();
    assert!(out.contains("kiss test: Planning ..."), "{out}");
    assert!(out.contains("PASS: tests/a.py::test_a"), "{out}");
    assert!(out.contains("✓ 1 passed"), "{out}");
    assert!(!out.contains("PASS (cached): 1 selectors"), "{out}");
    assert_eq!(report.exit_code, 0);
    assert!(!report.interrupted);
}

#[test]
fn shared_report_propagates_test_failure_exit_code() {
    let mut args = dry_args();
    args.dry_run = false;
    let report = run_kiss_test_report(args, |_a| RunTestOnceOutcome::Code(2));
    assert_eq!(report.exit_code, 2);
}

#[test]
fn shared_report_carries_structured_totals() {
    let mut args = dry_args();
    args.dry_run = false;
    let report = run_kiss_test_report_reuse(
        args,
        |_a| {
            crate::test_runner::emit_test_progress("PASS (cached): 2633 selectors");
            crate::test_runner::final_summary::print_final_test_summary(
                &crate::test_runner::final_summary::FinalTestSummary {
                    passed: 11816,
                    failed: 3,
                    failed_selectors: vec![
                        "tests/slow/ops/test_argus.py::test_argus_subscribe_counts_published_pings"
                            .into(),
                        "tests/slow/ops/test_ops.py::test_ops_eval_measurement_model".into(),
                    ],
                    timed_out_selectors: vec![
                        "tests/slow/ops/test_observability.py::test_observability".into(),
                    ],
                    max_passing_run_duration: std::time::Duration::ZERO,
                },
                std::time::Duration::from_secs_f64(69.33),
            );
            RunTestOnceOutcome::Code(1)
        },
        true,
        Some(std::path::Path::new(".")),
    );
    let totals = report.totals.expect("structured totals");
    assert_eq!(totals.passed, 11816);
    assert_eq!(totals.failed, 2);
    assert_eq!(totals.timed_out, 1);
}

#[test]
fn ready_target_report_after_tests_loads_as_ready() {
    use crate::bin_cli::args::TestInvocation;
    use crate::test_runner::target_request::{
        EnsurePolicy, materialize_target_report, workspace_request,
    };
    use crate::test_runner::test_mode_fixtures::{git_in, init_git};
    use crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors;

    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    std::fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    std::fs::write(
        tmp.path().join("tests/test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "seed"])
            .status()
            .unwrap()
            .success()
    );
    assert!(store_rust_workspace_selectors(tmp.path(), &[], &[]));

    let mut args = crate::test_runner::test_mode_fixtures::dry_run_cmd_args(
        TestInvocation::All,
        &[],
        1,
        Some(kiss::Language::Rust),
    );
    args.dry_run = false;
    let report = run_kiss_test_report_reuse(
        args,
        |_a| {
            record_rust_pass(tmp.path());
            let request = workspace_request(Some(kiss::Language::Rust), &[]);
            materialize_target_report(tmp.path(), &request, &EnsurePolicy::soft(false)).unwrap();
            assert!(
                crate::test_runner::target_request::load_ready_for_request(
                    tmp.path(),
                    &request,
                    crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
                )
                .is_some(),
                "published report must load as ready"
            );
            RunTestOnceOutcome::Code(0)
        },
        true,
        Some(tmp.path()),
    );
    assert_eq!(report.exit_code, 0);
}

#[test]
fn workspace_request_sorts_ignore_prefixes() {
    use crate::test_runner::target_request::{TargetFocus, workspace_request};
    let request = workspace_request(
        Some(kiss::Language::Rust),
        &["z".into(), "a".into(), "a".into()],
    );
    assert!(matches!(request.focus, TargetFocus::Workspace));
    assert_eq!(request.ignore, vec!["a".to_string(), "z".to_string()]);
}

/// What a Rust run that passed one test leaves behind.
fn record_rust_pass(repo: &std::path::Path) {
    assert!(
        crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
            repo,
            &[],
            &["t_ok".into()]
        )
    );
    crate::test_runner::lang_rust::test_records::store(
        repo,
        &[(
            "t_ok".into(),
            crate::test_runner::lang_iface::WitnessStatus::Passed,
        )],
    );
}
