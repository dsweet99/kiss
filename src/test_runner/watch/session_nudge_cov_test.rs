#![cfg(unix)]

use super::super::super::nudge_kind::NudgeInvocation;
use super::super::super::session_idle::{
    idle_cached_reply, try_reply_idle_nudge, LastReplies, QueuedCycle,
};
use super::super::*;
use super::{commit_a_py, py_dry_args, timeout_steps, NudgeScript};
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::{NudgeReplyMsg, NudgeRequestMsg};
use crate::test_runner::RunTestOnceOutcome;
use kiss::rust_llvm_cov_runner::WatchSuiteReport;
use std::path::Path;
use std::sync::{mpsc, Arc};
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
            reply.output.as_deref().is_none_or(str::is_empty),
            "coverage-only failure has no PASS/FAIL transcript; output={:?}",
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
                ..Default::default()
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
                ..Default::default()
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

#[test]
fn language_slices_omit_bilingual_engine_error() {
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS (cached): 2 selectors".into(),
        "kiss test: lang_collapsed python pass 2".into(),
        "PASS (cached): 3 selectors".into(),
        "kiss test: lang_collapsed rust pass 3".into(),
        "FAIL: tests/a.py::test_a (0.01s)".into(),
        "TIMEOUT: tests/a.py::test_b (0.01s)".into(),
        "✗ 5 passed · 1 failed · 1 timed out · 1s total · 0s max pass".into(),
    ]);
    let bilingual = NudgeReplyMsg {
        exit_code: 1,
        pid: 7,
        error: Some("error: kiss test: rust llvm-cov failed: InvalidRequest(\"stale\")".into()),
        output: Some("full recap".into()), idle_cache: None,
    };
    let mut last = LastReplies::for_repo(Path::new("/tmp"));
    last.store_named_language_slices(&suite, &bilingual);
    let py = last
        .get(Some(kiss::Language::Python))
        .expect("python slice");
    let rs = last.get(Some(kiss::Language::Rust)).expect("rust slice");
    assert!(py.error.is_none(), "python slice error={:?}", py.error);
    assert!(rs.error.is_none(), "rust slice error={:?}", rs.error);
    assert_eq!(py.exit_code, 124);
    assert_eq!(rs.exit_code, 0);
    assert_eq!(py.pid, 7);
    assert_eq!(rs.pid, 7);
    assert!(
        py.output.as_deref().is_some_and(|o| o.contains("failed")),
        "python recap={:?}",
        py.output
    );
    assert!(
        rs.output.as_deref().is_some_and(|o| o.contains("3 passed")),
        "rust recap={:?}",
        rs.output
    );
}

fn idle_queue(reply: mpsc::SyncSender<NudgeReplyMsg>) -> Option<QueuedCycle> {
    Some(QueuedCycle {
        replies: vec![(None, reply)],
        force: false,
        force_bad: false,
        metrics: false,
        invocation: NudgeInvocation::All,
        targets: Vec::new(),
        unscoped_force: false,
        lang_filter: None,
        ignore: Vec::new(),
        extra: Vec::new(),
        python_extra: Vec::new(),
        filter_override: false,
        next: None,
    })
}

#[test]
fn idle_cached_reply_strips_error_when_recap_output_is_present() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 124,
        pid: 1,
        error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
        output: Some("TIMEOUT (cached): 1 selectors\n".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 124);
    assert!(
        msg.error.is_none(),
        "idle recap must not replay engine error"
    );
    assert_eq!(
        msg.output.as_deref(),
        Some("TIMEOUT (cached): 1 selectors\n")
    );
}

#[test]
fn idle_cached_reply_keeps_error_when_output_is_empty() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("coverage gate failed".into()),
        output: None, idle_cache: None,
    });
    assert_eq!(msg.exit_code, 1);
    assert_eq!(msg.error.as_deref(), Some("coverage gate failed"));
}

#[test]
fn idle_cached_reply_keeps_coverage_gate_when_recap_present() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("coverage gate failed".into()),
        output: Some("✓ 3 passed · 0 failed · 0 timed out · 0.1s total · 0s max pass".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 1);
    assert_eq!(msg.error.as_deref(), Some("coverage gate failed"));
    assert!(msg
        .output
        .as_deref()
        .is_some_and(|o| o.contains("3 passed")));
}

#[test]
fn idle_cached_reply_strips_rslip_when_recap_present() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: rslip failed: InvalidRequest(\"x\")".into()),
        output: Some("FAIL (cached): 1 selectors\n✗ 1 passed · 1 failed · 0 timed out".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 1);
    assert!(
        msg.error.is_none(),
        "idle recap must not replay rslip: {:?}",
        msg.error
    );
}

#[test]
fn idle_cached_reply_strips_missing_population_when_recap_present() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: Rust runtime line coverage is missing population.".into()),
        output: Some("✓ 3 passed · 0 failed · 0 timed out".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 0);
    assert!(msg.error.is_none(), "{:?}", msg.error);
}

#[test]
fn idle_cached_reply_maps_pass_recap_to_zero_after_llvm_cov_strip() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
        output: Some("PASS (cached): 3 selectors\n✓ 3 passed · 0 failed · 0 timed out\n".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 0);
    assert!(msg.error.is_none(), "{:?}", msg.error);
}

#[test]
fn idle_cached_reply_keeps_exit_when_recap_has_coverage_violation() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: None,
        output: Some(
            "PASS (cached): test_lib.py::test_f\n\
             VIOLATION:test_coverage: codebase coverage 50% below 90% threshold\n\
             ✓ 1 passed · 0 failed · 0 timed out\n"
                .into(),
        ), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 1);
    assert!(msg.error.is_none());
}

#[test]
fn idle_nudge_omits_engine_error_when_recap_is_cached() {
    let mut last = LastReplies::for_repo(Path::new("/tmp"));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 124,
            pid: 9,
            error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
            output: Some("TIMEOUT (cached): 1 selectors\n✗ 1 passed · 0 failed · 1 timed out".into()), idle_cache: None,
    },
    );
    let (tx, rx) = mpsc::sync_channel(1);
    let mut queued = idle_queue(tx);
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    let reply = rx.try_recv().expect("idle reply");
    assert_eq!(reply.exit_code, 124);
    assert!(reply.error.is_none(), "idle nudge error={:?}", reply.error);
    assert_eq!(reply.pid, 9);
}

#[test]
fn idle_cached_reply_remaps_timeout_recap_exit_to_124() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
        output: Some("TIMEOUT (cached): 1 selectors\n✗ 1 passed · 0 failed · 1 timed out\n".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 124);
    assert!(msg.error.is_none(), "{:?}", msg.error);
}

#[test]
fn idle_cached_reply_keeps_fail_exit_without_timeout_line() {
    let msg = idle_cached_reply(NudgeReplyMsg {
        exit_code: 1,
        pid: 1,
        error: Some("error: kiss test: rust llvm-cov failed: stale".into()),
        output: Some("FAIL (cached): 1 selectors\n✗ 1 passed · 1 failed · 0 timed out\n".into()), idle_cache: None,
    });
    assert_eq!(msg.exit_code, 1);
    assert!(msg.error.is_none(), "{:?}", msg.error);
}
