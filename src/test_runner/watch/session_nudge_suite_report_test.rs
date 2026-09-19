use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::{RunTestOnceOutcome, WatchCoverageResult, run_kiss_test_report_reuse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn watch_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
    args
}

fn emit_rslip_then_summary() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
    crate::test_runner::emit_test_progress("kiss test: tests_remaining=2");
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 3,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

#[test]
fn watch_to_waiting_then_oneshot_skips_rslip() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let mut src = NudgeScript {
        steps: timeout_steps(4),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        watch_args(),
        Duration::from_millis(20),
        tmp.path(),
        &mut src,
        None,
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            emit_rslip_then_summary()
        },
        |_| WatchCoverageResult::ok(0),
    );
    assert_eq!(code, 1);
    assert_eq!(watch_runs.load(Ordering::SeqCst), 1);
    assert!(tmp.path().join(".kiss").join("suite_report.json").exists());
    let oneshot = run_kiss_test_report_reuse(
        watch_args(),
        |_| panic!("watch-dead oneshot must not re-enter the engine"),
        |_| panic!("watch-dead oneshot must not run coverage"),
        true,
        Some(tmp.path()),
    );
    assert_eq!(oneshot.exit_code, 0);
    let replayed = oneshot.output.unwrap_or_default();
    assert!(replayed.contains("3 passed"), "{replayed}");
    assert!(!replayed.contains("rslip prepared"), "{replayed}");
    assert!(!replayed.contains("tests_remaining"), "{replayed}");
}
