#![cfg(unix)]

use super::super::*;
use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::NudgeRequestMsg;
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn watch_cycle_runs_cov_after_tests_and_propagates_cov_exit() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let (tx, rx) = mpsc::sync_channel::<NudgeRequest>(4);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_tx,
        })
        .unwrap();
        let reply = reply_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(reply.exit_code, 1);
        assert_eq!(reply.error.as_deref(), Some("coverage gate failed"));
        assert!(
            reply.output.is_none(),
            "watcher replies must not carry stdout; output={:?}",
            reply.output
        );
    });
    let cov_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cov_calls_c = Arc::clone(&cov_calls);
    let mut src = NudgeScript {
        steps: timeout_steps(8),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        |_a| RunTestOnceOutcome::Code(0),
        move |_a| {
            cov_calls_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            WatchCoverageResult::failed(1, "coverage gate failed")
        },
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    assert!(
        cov_calls.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "cov must run after tests exit 0"
    );
}

#[test]
fn watch_cycle_skips_cov_when_tests_fail() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let (tx, rx) = mpsc::sync_channel::<NudgeRequest>(4);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_tx,
        })
        .unwrap();
        let reply = reply_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(reply.exit_code, 7);
        assert!(reply.error.is_none());
    });
    let cov_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cov_calls_c = Arc::clone(&cov_calls);
    let mut src = NudgeScript {
        steps: timeout_steps(8),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        |_a| RunTestOnceOutcome::Code(7),
        move |_a| {
            cov_calls_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            WatchCoverageResult::ok(0)
        },
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        cov_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "cov must not run when tests fail"
    );
}

#[test]
fn watch_cycle_interrupted_during_tests_replies_130_without_cov() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let (tx, rx) = mpsc::sync_channel::<NudgeRequest>(4);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,
                force_bad: false,
                metrics: false,
            },
            reply: reply_tx,
        })
        .unwrap();
        let reply = reply_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(reply.exit_code, 130);
        assert!(reply.error.is_none());
    });
    let cov_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cov_calls_c = Arc::clone(&cov_calls);
    let cycle_n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cycle_n_c = Arc::clone(&cycle_n);
    let mut src = NudgeScript {
        steps: timeout_steps(8),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_a| {
            if cycle_n_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                RunTestOnceOutcome::Code(0)
            } else {
                RunTestOnceOutcome::Interrupted
            }
        },
        move |_a| {
            cov_calls_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            WatchCoverageResult::ok(0)
        },
    );
    sender.join().unwrap();
    assert_eq!(code, 130);
    assert_eq!(
        cov_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "cov runs after first cycle only; interrupted test cycle must not start cov"
    );
}

#[test]
fn watch_cycle_interrupted_during_cov_replies_130() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let (tx, rx) = mpsc::sync_channel::<NudgeRequest>(4);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,
                force_bad: false,
                metrics: false,
            },
            reply: reply_tx,
        })
        .unwrap();
        let reply = reply_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(reply.exit_code, 130);
        assert!(reply.error.is_none());
    });
    let cycle_n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cycle_n_c = Arc::clone(&cycle_n);
    let mut src = NudgeScript {
        steps: timeout_steps(8),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        |_a| RunTestOnceOutcome::Code(0),
        move |_a| {
            if cycle_n_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                WatchCoverageResult::ok(0)
            } else {
                WatchCoverageResult::interrupted()
            }
        },
    );
    sender.join().unwrap();
    assert_eq!(code, 130);
}
