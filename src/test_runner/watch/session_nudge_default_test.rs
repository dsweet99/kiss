#![cfg(unix)]

use super::super::*;
use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::NudgeRequestMsg;
use crate::test_runner::watch::event_source::NormalizedWatchEvent;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn default_nudge_while_settling_runs_new_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let file = commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(40));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_tx,
        })
        .unwrap();
        let _ = reply_rx.recv_timeout(Duration::from_secs(5));
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let mut steps = VecDeque::new();
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(12));
    let mut src = NudgeScript { steps };
    let code = run_watch_loop_with(
        py_dry_args(),
        Duration::from_secs(30),
        tmp.path(),
        &mut src,
        Some(&rx),
        |_args| {
            tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a file edit starts one cycle; a plain nudge does not start another"
    );
}

#[test]
fn config_change_reruns_before_answering_client() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let cfg = tmp.path().join(".kissconfig");
    std::fs::write(&cfg, "[python]\n[rust]\n").unwrap();

    let forces = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(80));
        std::fs::write(&cfg, "[python]\n[rust]\n# drifted\n").unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_tx,
        })
        .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let forces_run = std::sync::Arc::clone(&forces);
    let repo = tmp.path().to_path_buf();
    let mut steps = VecDeque::new();
    steps.extend(timeout_steps(80));
    let mut src = NudgeScript { steps };
    let mut args = py_dry_args();
    args.dry_run = false;
    let _code = run_watch_loop_with(
        args,
        Duration::from_secs(30),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            forces_run.lock().unwrap().push(cycle_args.force_rerun);
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let status = if n == 0 {
                crate::test_runner::target_request::EffectiveStatus::Pass
            } else {
                crate::test_runner::target_request::EffectiveStatus::Fail
            };
            super::publish_workspace_rows(
                &repo,
                &[("python", "tests/a.py::test_a", status)],
                if n == 0 { 0 } else { 1 },
            );
            RunTestOnceOutcome::Code(if n == 0 { 0 } else { 1 })
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let reply = sender.join().unwrap();
    let forces = forces.lock().unwrap();
    assert!(
        forces.len() >= 2,
        "a config change starts another cycle; forces={forces:?}"
    );
    assert!(
        !forces[0],
        "the startup cycle is not a config rerun; forces={forces:?}"
    );
    assert!(
        forces[1],
        "the cycle after a config change reruns instead of reusing cached results; forces={forces:?}"
    );
    assert_eq!(
        reply.warning.as_deref(),
        Some("kiss test-watch: running with outdated configs")
    );
    let out = reply.output.unwrap_or_default();
    assert!(
        out.contains("failed") || out.contains("FAIL"),
        "the client is answered from the rerun, not the pre-change cache; out={out:?}"
    );
}
