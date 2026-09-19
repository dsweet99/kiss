use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::test_runner::test_mode_fixtures::{init_git, with_cwd};
use crate::test_runner::{RunTestOnceOutcome, WatchCoverageResult, run_kiss_test_report};

fn live_all_args() -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

fn python_all_args() -> RunTestCmdArgs<'static> {
    let mut args = live_all_args();
    args.lang_filter = Some(kiss::Language::Python);
    args
}

fn rust_all_args() -> RunTestCmdArgs<'static> {
    let mut args = live_all_args();
    args.lang_filter = Some(kiss::Language::Rust);
    args
}

fn emit_bilingual_run() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
    crate::test_runner::emit_test_progress("kiss test: tests_remaining=2");
    {
        let _guard =
            kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Python);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    {
        let _guard = kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Rust);
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
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
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
    std::fs::write(tmp.path().join("t.py"), "def test_a():\n    assert True\n").unwrap();
}

#[test]
fn all_hit_replays_compact_recap_without_running() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 0);
        let second = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("source-stable oneshot must not re-enter the engine")
            },
            |_a| panic!("source-stable oneshot must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(second.exit_code, 0);
        assert_eq!(second.totals.as_ref().map(|t| t.passed), Some(3));
        let replayed = second.output.unwrap_or_default();
        assert!(replayed.contains("3 passed"), "{replayed}");
        assert!(!replayed.contains("rslip prepared"), "{replayed}");
        assert!(!replayed.contains("tests_remaining"), "{replayed}");
    });
}

#[test]
fn source_edit_misses_and_reruns() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        std::fs::write(tmp.path().join("t.py"), "def test_a():\n    assert False\n").unwrap();
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
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
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b + 0 }\n",
        )
        .unwrap();
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
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
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        let mut forced = live_all_args();
        forced.force_bad = true;
        let _ = run_kiss_test_report(
            forced,
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    });
}

fn assert_edit_misses(edit: impl FnOnce(&std::path::Path)) {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        edit(tmp.path());
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
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
        std::fs::write(root.join("rust/pkg/pyproject.toml"), "[project]\nname='x'\n").unwrap();
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
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        std::fs::write(&custom, "[test]\nnum_jobs = 2\n").unwrap();
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
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
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname='x'\n").unwrap();
        let second = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("gitignored pyproject.toml must not invalidate the suite recap")
            },
            |_a| panic!("gitignored pyproject.toml must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(second.exit_code, 0);
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
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        std::fs::write(tmp.path().join("foo.inc"), "// fragment\n").unwrap();
        let second = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("gitignored .inc must not invalidate the suite recap")
            },
            |_a| panic!("gitignored .inc must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(second.exit_code, 0);
    });
}

#[test]
fn unscoped_then_python_then_unscoped_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_bilingual_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 0);
        let python = run_kiss_test_report(
            python_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("lang-scoped oneshot must not re-enter the engine")
            },
            |_a| panic!("lang-scoped oneshot must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(python.exit_code, 0);
        let python_out = python.output.unwrap_or_default();
        assert!(python_out.contains("2 passed"), "{python_out}");
        assert!(!python_out.contains("rslip prepared"), "{python_out}");
        assert!(!python_out.contains("tests_remaining"), "{python_out}");
        let third = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("source-stable oneshot must not re-enter the engine")
            },
            |_a| panic!("source-stable oneshot must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(third.exit_code, 0);
        let replayed = third.output.unwrap_or_default();
        assert!(replayed.contains("5 passed"), "{replayed}");
        assert!(!replayed.contains("rslip prepared"), "{replayed}");
        assert!(!replayed.contains("tests_remaining"), "{replayed}");
    });
}

fn emit_python_only_covering_miss() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
    {
        let _guard =
            kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Python);
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

fn emit_covering_abort() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
    {
        let _guard =
            kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Python);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    {
        let _guard = kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Rust);
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
    RunTestOnceOutcome::EngineError(
        "error: kiss test: rust llvm-cov failed: InvalidRequest(\"95 cargo-llvm-cov processes live (cap 84); aborting instrumented nextest\")".into(),
    )
}

#[test]
fn unscoped_covering_miss_without_rust_counts_does_not_wipe_bilingual() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n")
        .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_bilingual_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 0);
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n// covering-miss\n",
        )
        .unwrap();
        let _ = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_python_only_covering_miss()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        let retry = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("unattributed rust covering miss must not replace the bilingual recap")
            },
            |_a| panic!("unattributed rust covering miss must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(retry.exit_code, 0);
        let replayed = retry.output.unwrap_or_default();
        assert!(
            replayed.contains("5 passed"),
            "bilingual rust counts must survive an unscoped covering miss with rust 0\n{replayed}"
        );
    });
}

#[test]
fn rust_covering_abort_does_not_persist_partial_recap() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n")
        .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_bilingual_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 0);
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n// covering-abort\n",
        )
        .unwrap();
        let abort = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_covering_abort()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(abort.exit_code, 1);
        let retry = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_bilingual_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(
            runs.load(Ordering::SeqCst),
            3,
            "covering abort must not persist a recap for the edited rust digest"
        );
        assert_eq!(retry.exit_code, 0);
        let replayed = retry.output.unwrap_or_default();
        assert!(
            replayed.contains("5 passed"),
            "retry after covering abort must run the suite, not replay rust 2 / 4 passed\n{replayed}"
        );
    });
}

#[test]
fn rust_scoped_unattributed_persist_does_not_wipe_bilingual() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    seed_repo(&tmp);
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n")
        .unwrap();
    with_cwd(tmp.path(), || {
        let runs = AtomicUsize::new(0);
        let first = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_bilingual_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(first.exit_code, 0);
        let mut rust_forced = rust_all_args();
        rust_forced.force_rerun = true;
        let _ = run_kiss_test_report(
            rust_forced,
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                emit_sample_run()
            },
            |_a| WatchCoverageResult::ok(0),
        );
        let third = run_kiss_test_report(
            live_all_args(),
            |_a| {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("unattributed rust persist must not replace the bilingual recap")
            },
            |_a| panic!("unattributed rust persist must not run coverage"),
        );
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert_eq!(third.exit_code, 0);
        let replayed = third.output.unwrap_or_default();
        assert!(
            replayed.contains("5 passed"),
            "bilingual rust counts must survive an unattributed rust-scoped persist\n{replayed}"
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
        args.invocation = TestInvocation::Targets(vec!["t.py".into()]);
        let _ = run_kiss_test_report(
            args,
            |_a| emit_sample_run(),
            |_a| WatchCoverageResult::ok(0),
        );
        assert!(!tmp.path().join(".kiss").join("suite_report.json").exists());
    });
}
