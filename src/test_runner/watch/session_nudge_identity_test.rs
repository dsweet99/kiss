use super::{NudgeScript, commit_a_py, publish_pass_count, py_dry_args, timeout_steps};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::{NudgeRequest, NudgeRequestMsg};
use crate::test_runner::{RunTestOnceOutcome, WatchCoverageResult, run_kiss_test_report_reuse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn all_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    args
}

fn other_ignore_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = all_args();
    args.set_ignore(Box::leak(vec!["other_prefix".into()].into_boxed_slice()));
    args
}

fn emit_counts(passed: usize) -> RunTestOnceOutcome {
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

#[test]
fn mismatched_ignore_does_not_replay_other_identity_recap() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let persist_runs = AtomicUsize::new(0);
    let first = run_kiss_test_report_reuse(
        all_args(),
        |_a| {
            persist_runs.fetch_add(1, Ordering::SeqCst);
            emit_counts(5)
        },
        |_a| WatchCoverageResult::ok(0),
        true,
        Some(tmp.path()),
    );
    assert_eq!(persist_runs.load(Ordering::SeqCst), 1);
    assert_eq!(first.exit_code, 1);
    assert_eq!(
        first.error.as_deref(),
        Some("target membership is not proven complete")
    );
    publish_pass_count(tmp.path(), 5);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let repo = tmp.path().to_path_buf();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_tx,
        })
        .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        other_ignore_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            publish_pass_count(&repo, 9);
            emit_counts(9)
        },
        |_| WatchCoverageResult::ok(0),
    );
    let reply = sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(watch_runs.load(Ordering::SeqCst), 1);
    let out = reply.output.unwrap_or_default();
    assert!(
        out.contains("9 passed") && !out.contains("5 passed"),
        "other-ignore watch must not idle-reply the empty-ignore recap; out={out}"
    );
}

#[test]
fn idle_after_ignore_override_keeps_session_identity_recap() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let persist_runs = AtomicUsize::new(0);
    let first = run_kiss_test_report_reuse(
        all_args(),
        |_a| {
            persist_runs.fetch_add(1, Ordering::SeqCst);
            emit_counts(5)
        },
        |_a| WatchCoverageResult::ok(0),
        true,
        Some(tmp.path()),
    );
    assert_eq!(persist_runs.load(Ordering::SeqCst), 1);
    assert_eq!(first.exit_code, 1);
    assert_eq!(
        first.error.as_deref(),
        Some("target membership is not proven complete")
    );
    publish_pass_count(tmp.path(), 5);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_over_tx, reply_over_rx) = mpsc::sync_channel(1);
    let (reply_idle_tx, reply_idle_rx) = mpsc::sync_channel(1);
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                target_request: crate::test_runner::target_request::workspace_request(
                    None,
                    &["other_prefix".into()],
                ),
                ..Default::default()
            },
            reply: reply_over_tx,
        })
        .unwrap();
        let over = reply_over_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_idle_tx,
        })
        .unwrap();
        let idle = reply_idle_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (over, idle)
    });
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        all_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            emit_counts(9)
        },
        |_| WatchCoverageResult::ok(0),
    );
    let (over, idle) = sender.join().unwrap();
    assert_eq!(code, 1);
    let over_out = over.output.as_deref().unwrap_or("");
    let idle_out = idle.output.unwrap_or_default();
    assert_eq!(over.exit_code, 1);
    assert!(
        over.error.as_deref().is_some_and(|err| {
            err.contains("incomplete evidence") || err.contains("not proven complete")
        }) || over_out.contains("9 passed")
            || over_out.contains("report members="),
        "ignore-override is a distinct identity and must not slice the session report; over={over:?}"
    );
    assert!(
        idle_out.contains("5 passed") && !idle_out.contains("9 passed"),
        "session-identity idle must keep the matching store recap; out={idle_out}"
    );
}
