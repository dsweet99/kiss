#![cfg(unix)]

use super::super::*;
use super::{commit_a_py, py_dry_args, timeout_steps, NudgeScript};
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::NudgeRequestMsg;
use crate::test_runner::watch::event_source::NormalizedWatchEvent;
use crate::test_runner::RunTestOnceOutcome;
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
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .exit_code,
            0
        );
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
        2,
        "default nudge while settling must run the changed files"
    );
}
