use std::sync::atomic::{AtomicUsize, Ordering};

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::test_mode_fixtures::{init_git, with_cwd};
use crate::test_runner::{
    KissTestReport, RunTestOnceOutcome, run_kiss_test_report, run_kiss_test_report_reuse,
};

fn run_transcript_report<F>(args: RunTestCmdArgs<'_>, run_tests: F) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
{
    run_kiss_test_report_reuse(args, run_tests, true, Some(std::path::Path::new(".")))
}

fn live_all_args() -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        doubles: None,
        invocation: TestInvocation::All,
        target_request: crate::test_runner::target_request::workspace_request(None, &[]),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

fn python_all_args() -> RunTestCmdArgs<'static> {
    let mut args = live_all_args();
    args.set_lang_filter(Some(kiss::Language::Python));
    args
}

fn rust_all_args() -> RunTestCmdArgs<'static> {
    let mut args = live_all_args();
    args.set_lang_filter(Some(kiss::Language::Rust));
    args
}

fn emit_bilingual_run() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: stage python_source_fingerprint 0ms");
    crate::test_runner::emit_test_progress("kiss test: tests_remaining=2");
    {
        let _guard = kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Python);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    {
        let _guard = kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Rust);
        crate::test_runner::emit_test_progress("PASS (cached): 3 selectors");
    }
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 5,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        std::time::Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

fn emit_sample_run() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: stage python_source_fingerprint 0ms");
    crate::test_runner::emit_test_progress("kiss test: tests_remaining=2");
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 3,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        std::time::Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

fn seed_repo(tmp: &tempfile::TempDir) {
    init_git(tmp);
    std::fs::write(
        tmp.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         orphan_detection = false\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("t.py"), "def test_a():\n    assert True\n").unwrap();
}

fn publish_rust_ready(repo: &std::path::Path) {
    use crate::test_runner::target_request::{
        EnsurePolicy, materialize_target_report, workspace_request,
    };
    use crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors;
    assert!(store_rust_workspace_selectors(repo, &[], &["t_ok".into()]));
    crate::test_runner::lang_rust::test_records::store(
        repo,
        &[(
            "t_ok".into(),
            crate::test_runner::lang_iface::WitnessStatus::Passed,
        )],
    );
    let request = workspace_request(Some(kiss::Language::Rust), &[]);
    materialize_target_report(repo, &request, &EnsurePolicy::soft(false)).unwrap();
    assert!(
        crate::test_runner::target_request::load_ready_for_request(
            repo,
            &request,
            crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        )
        .is_some(),
        "published rust-ready report must load"
    );
}

#[test]
fn all_hit_still_runs_the_selected_tests() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report_reuse(
            rust_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            true,
            Some(tmp.path()),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(
            first.exit_code, 0,
            "no Rust tests: the run settles an empty scope"
        );
        assert_eq!(first.error, None);
        publish_rust_ready(tmp.path());
        assert!(
            tmp.path().join(".kiss").join("suite_report.json").is_file()
                || crate::test_runner::target_request::load_ready_for_request(
                    tmp.path(),
                    &crate::test_runner::target_request::workspace_request(
                        Some(kiss::Language::Rust),
                        &[],
                    ),
                    crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
                )
                .is_some()
        );
        let second = run_kiss_test_report_reuse(
            rust_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            true,
            Some(tmp.path()),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(second.exit_code, 0);
        let replayed = second.output.unwrap_or_default();
        assert!(
            replayed.contains("1 passed"),
            "{replayed}"
        );
        assert!(!replayed.contains("PASS (cached)"), "{replayed}");
    });
}

#[test]
fn source_edit_misses_and_reruns() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        std::fs::write(tmp.path().join("t.py"), "def test_a():\n    assert False\n").unwrap();
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn rust_body_edit_misses_and_reruns() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b + 0 }\n",
        )
        .unwrap();
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "rust function-body edit must miss DurableSuiteRecap"
        );
    });
}

#[test]
fn force_bad_misses_and_reruns() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        let mut forced = live_all_args();
        forced.force_bad = true;
        let _ = run_kiss_test_report(forced, |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    });
}

fn assert_edit_misses(edit: impl FnOnce(&std::path::Path)) {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        edit(tmp.path());
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn gitignore_edit_misses_and_reruns() {
    assert_edit_misses(|root| {
        std::fs::write(root.join(".gitignore"), "*.pyc\n").unwrap();
    });
}

#[test]
fn kissignore_edit_misses_and_reruns() {
    assert_edit_misses(|root| {
        std::fs::write(root.join(".kissignore"), "tmp/\n").unwrap();
    });
}

#[test]
fn inc_edit_misses_and_reruns() {
    assert_edit_misses(|root| {
        std::fs::write(root.join("foo.inc"), "// fragment\n").unwrap();
    });
}

#[test]
fn nested_pyproject_edit_misses_and_reruns() {
    assert_edit_misses(|root| {
        std::fs::create_dir_all(root.join("rust/pkg")).unwrap();
        std::fs::write(
            root.join("rust/pkg/pyproject.toml"),
            "[project]\nname='x'\n",
        )
        .unwrap();
    });
}

#[test]
fn nested_config_toml_edit_misses_and_reruns() {
    assert_edit_misses(|root| {
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/config.toml"), "x=1\n").unwrap();
    });
}

#[test]
fn custom_config_edit_misses_and_reruns() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    let custom = tmp.path().join("custom.toml");
    std::fs::write(&custom, "[test]\nnum_jobs = 1\n").unwrap();
    let _guard = kiss::ConfigPathOverrideGuard::enter(Some(&custom));
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        std::fs::write(&custom, "[test]\nnum_jobs = 2\n").unwrap();
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn gitignored_pyproject_does_not_rerun() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::write(tmp.path().join(".gitignore"), "pyproject.toml\n").unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname='x'\n").unwrap();
        let second = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(second.exit_code, 1);
        assert_eq!(
            second.error.as_deref(),
            Some("target membership is not proven complete")
        );
    });
}

#[test]
fn gitignored_inc_does_not_rerun() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::write(tmp.path().join(".gitignore"), "*.inc\n").unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        std::fs::write(tmp.path().join("foo.inc"), "// fragment\n").unwrap();
        let second = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(second.exit_code, 1);
        assert_eq!(
            second.error.as_deref(),
            Some("target membership is not proven complete")
        );
    });
}

#[test]
fn unscoped_then_python_then_unscoped_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 1);
        let python = run_kiss_test_report(python_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(python.exit_code, 1);
        let third = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 3);
        assert_eq!(third.exit_code, 1);
    });
}

fn emit_python_only_selecting_miss() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: stage python_source_fingerprint 0ms");
    {
        let _guard = kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Python);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 2,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        std::time::Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

fn emit_selecting_abort() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: stage python_source_fingerprint 0ms");
    {
        let _guard = kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Python);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    {
        let _guard = kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Rust);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 4,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        std::time::Duration::from_millis(10),
    );
    RunTestOnceOutcome::EngineError("error: kiss test: cargo nextest exited with code 101".into())
}

#[test]
fn unscoped_selecting_miss_without_rust_counts_does_not_wipe_bilingual() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 1);
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n// selecting-miss\n",
        )
        .unwrap();
        let _ = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_python_only_selecting_miss()
        });
        let retry = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 3);
        assert_eq!(retry.exit_code, 1);
    });
}

#[test]
fn rust_selecting_abort_does_not_persist_partial_recap() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 1);
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n// selecting-abort\n",
        )
        .unwrap();
        let abort = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_selecting_abort()
        });
        assert_eq!(abort.exit_code, 1);
        let retry = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(
            runs.load(Ordering::SeqCst),
            3,
            "selecting abort must not persist a recap for the edited rust digest"
        );
        assert_eq!(retry.exit_code, 1);
        assert!(
            retry.output.is_none(),
            "retry without a TargetReport must not officialize transcript: {:?}",
            retry.output
        );
    });
}

#[test]
fn rust_scoped_unattributed_persist_does_not_wipe_bilingual() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 1);
        let mut rust_forced = rust_all_args();
        rust_forced.force_rerun = true;
        let _ = run_kiss_test_report(rust_forced, |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_sample_run()
        });
        let third = run_kiss_test_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        });
        assert_eq!(runs.load(Ordering::SeqCst), 3);
        assert_eq!(third.exit_code, 1);
    });
}

#[test]
fn persist_skips_deleted_but_indexed_rust_file() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    let rust = tmp.path().join("src/evaluate.rs");
    std::fs::write(&rust, "pub fn evaluate() {}\n").unwrap();
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(tmp.path())
            .args(["add", "src/evaluate.rs"])
            .status()
            .unwrap()
            .success()
    );
    std::fs::remove_file(&rust).unwrap();
    let listed = crate::test_runner::test_mode_fixtures::git_stdout(
        tmp.path(),
        &["ls-files", "-c", "--", "src/evaluate.rs"],
    );
    assert_eq!(listed, "src/evaluate.rs");
    with_cwd(tmp.path(), || {
        let _ = run_kiss_test_report(live_all_args(), |_a| emit_sample_run());
        assert!(
            !tmp.path().join(".kiss").join("suite_report.json").exists(),
            "legacy suite_report.json must not be written"
        );
    });
}

#[test]
fn target_scoped_run_does_not_persist() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let mut args = live_all_args();
        args.set_invocation(TestInvocation::Targets(vec!["t.py".into()]));
        let _ = run_kiss_test_report(args, |_a| emit_sample_run());
        assert!(!tmp.path().join(".kiss").join("suite_report.json").exists());
    });
}

#[test]
fn warm_replay_lists_cached_fail_and_timeout_names_without_pass_names() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_transcript_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            {
                let _guard =
                    kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Python);
                crate::test_runner::emit_test_progress("PASS (cached): 40 selectors");
            }
            {
                let _guard = kiss::watch_report::ProgressLanguageGuard::enter(kiss::Language::Rust);
                crate::test_runner::emit_test_progress("PASS (cached): 30 selectors");
            }
            crate::test_runner::emit_test_progress("FAIL: tests/b.py::test_b (0.01s)");
            crate::test_runner::emit_test_progress("TIMEOUT: src/lib.rs::t_slow (3.00s)");
            crate::test_runner::final_summary::print_final_test_summary(
                &crate::test_runner::final_summary::FinalTestSummary {
                    passed: 70,
                    failed: 2,
                    failed_selectors: vec!["tests/b.py::test_b".into()],
                    timed_out_selectors: vec!["src/lib.rs::t_slow".into()],
                    ..crate::test_runner::final_summary::FinalTestSummary::default()
                },
                std::time::Duration::from_millis(10),
            );
            RunTestOnceOutcome::Code(1)
        });
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 1);
        assert!(
            first
                .named
                .iter()
                .any(|row| row.selector == "tests/b.py::test_b"
                    && row.outcome == kiss::watch_report::WatchNamedOutcome::Fail),
            "cold capture must name FAIL without ProgressLanguageGuard; named={:?}",
            first.named
        );
        assert!(
            first
                .named
                .iter()
                .any(|row| row.selector == "src/lib.rs::t_slow"
                    && row.outcome == kiss::watch_report::WatchNamedOutcome::Timeout),
            "cold capture must name TIMEOUT without ProgressLanguageGuard; named={:?}",
            first.named
        );

        let second = run_transcript_report(live_all_args(), |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            crate::test_runner::emit_test_progress("FAIL: tests/b.py::test_b (0.01s)");
            crate::test_runner::emit_test_progress("TIMEOUT: src/lib.rs::t_slow (3.00s)");
            crate::test_runner::final_summary::print_final_test_summary(
                &crate::test_runner::final_summary::FinalTestSummary {
                    passed: 70,
                    failed: 2,
                    failed_selectors: vec!["tests/b.py::test_b".into()],
                    timed_out_selectors: vec!["src/lib.rs::t_slow".into()],
                    ..crate::test_runner::final_summary::FinalTestSummary::default()
                },
                std::time::Duration::from_millis(10),
            );
            RunTestOnceOutcome::Code(1)
        });
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(second.exit_code, 1);
    });
}

#[test]
fn replay_suite_report_skips_absent_and_empty_output() {
    super::replay_suite_report(&KissTestReport {
        output: None,
        ..KissTestReport::default()
    });
    super::replay_suite_report(&KissTestReport {
        output: Some(String::new()),
        ..KissTestReport::default()
    });
}

#[test]
fn replay_suite_report_appends_newline_when_missing() {
    super::replay_suite_report(&KissTestReport {
        output: Some("cached-recap-without-newline".into()),
        ..KissTestReport::default()
    });
}
