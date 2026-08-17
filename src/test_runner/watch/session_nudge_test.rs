#![cfg(unix)]

use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use super::super::*;
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::NudgeRequestMsg;
use crate::test_runner::watch::event_source::{NormalizedWatchEvent, RecvTimeout};
use std::collections::VecDeque;
use std::env;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

fn send_nudge_after(
    delay: Duration,
    msg: NudgeRequestMsg,
) -> (
    mpsc::Receiver<NudgeRequest>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(delay);
        tx.send(NudgeRequest {
            msg,
            reply: reply_tx,
        })
        .unwrap();
        let _ = reply_rx.recv_timeout(Duration::from_secs(5));
    });
    (rx, sender)
}

#[test]
fn nudge_while_idle_runs_without_long_wait() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let (rx, sender) = send_nudge_after(Duration::from_millis(30), NudgeRequestMsg::default());
    let mut src = NudgeScript {
        steps: timeout_steps(7),
    };
    let t0 = Instant::now();
    let code = run_watch_loop(
        py_dry_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
    );
    let elapsed = t0.elapsed();
    sender.join().unwrap();
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
    assert!(elapsed < Duration::from_secs(5), "elapsed={elapsed:?}");
}

#[test]
fn nudge_while_waiting_skips_settle() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let file = commit_a_py(&tmp);
    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let (rx, sender) = send_nudge_after(
        Duration::from_millis(20),
        NudgeRequestMsg {
            force: true,
            force_bad: false,
            metrics: true,
        },
    );
    let mut steps = VecDeque::new();
    steps.push_back(Err(RecvTimeout::Timeout));
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(4));
    let mut src = NudgeScript { steps };
    let t0 = Instant::now();
    let code = run_watch_loop(
        py_dry_args(),
        Duration::from_secs(30),
        tmp.path(),
        &mut src,
        Some(&rx),
    );
    let elapsed = t0.elapsed();
    sender.join().unwrap();
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
    assert!(elapsed < Duration::from_secs(5), "elapsed={elapsed:?}");
}

#[test]
fn forwarded_force_applies_to_queued_cycle_then_clears() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r1, _w1) = mpsc::sync_channel(1);
    let (r2, _w2) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,
            force_bad: true,
            metrics: true,
        },
        reply: r1,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r2,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("coalesced");
    assert!(q.force && q.force_bad && q.metrics);
    assert_eq!(q.replies.len(), 2);
    let base = py_dry_args();
    assert!(!base.force_rerun && !base.force_bad && !base.metrics);
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let (cycle1, replies) = take_queued_cycle_args(&live, &mut queued);
    assert!(queued.is_none(), "queue consumed");
    assert!(cycle1.force_rerun && cycle1.force_bad && cycle1.metrics);
    assert_eq!(replies.len(), 2);
    let (cycle2, replies2) = take_queued_cycle_args(&live, &mut queued);
    assert!(!cycle2.force_rerun && !cycle2.force_bad && !cycle2.metrics);
    assert!(replies2.is_empty());
}

#[test]
fn overlapping_pre_start_nudges_share_one_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r1_tx, r1_rx) = mpsc::sync_channel(1);
    let (r2_tx, r2_rx) = mpsc::sync_channel(1);
    let cycles = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cycles_nudge = std::sync::Arc::clone(&cycles);
    let sender = std::thread::spawn(move || {
        while cycles_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: r1_tx,
        })
        .unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: r2_tx,
        })
        .unwrap();
        let a = r1_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let b = r2_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(a.exit_code, 0);
        assert_eq!(b.exit_code, 0);
    });

    let cycles_run = std::sync::Arc::clone(&cycles);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let code = run_watch_loop_with(
        py_dry_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        |_args| {
            cycles_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        cycles.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "overlapping pre-start nudges must share one cycle"
    );
}

fn spawn_force_nudge_during_barrier(
    entered: std::sync::Arc<std::sync::Barrier>,
    release: std::sync::Arc<std::sync::Barrier>,
) -> (
    mpsc::Receiver<NudgeRequest>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        entered.wait();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,
                force_bad: false,
                metrics: false,
            },
            reply: reply_tx,
        })
        .unwrap();
        release.wait();
        assert_eq!(reply_rx.recv_timeout(Duration::from_secs(5)).unwrap().exit_code, 0);
    });
    (rx, sender)
}

fn record_force_and_block_first(
    args: &RunTestCmdArgs<'_>,
    seen: &std::sync::Mutex<Vec<bool>>,
    entered: &std::sync::Barrier,
    release: &std::sync::Barrier,
) -> RunTestOnceOutcome {
    let n = {
        let mut v = seen.lock().unwrap();
        v.push(args.force_rerun);
        v.len()
    };
    if n == 1 {
        entered.wait();
        release.wait();
    }
    RunTestOnceOutcome::Code(0)
}

#[test]
fn nudge_while_cycle_in_flight_runs_second_cycle_before_reply() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
    let release = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (rx, sender) = spawn_force_nudge_during_barrier(Arc::clone(&entered), Arc::clone(&release));
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_c = Arc::clone(&seen);
    let entered_c = Arc::clone(&entered);
    let release_c = Arc::clone(&release);
    let mut src = NudgeScript {
        steps: timeout_steps(8),
    };
    let code = run_watch_loop_with(
        py_dry_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |args| record_force_and_block_first(&args, &seen_c, &entered_c, &release_c),
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    let flags = seen.lock().unwrap().clone();
    assert!(flags.len() >= 2, "flags={flags:?}");
    assert!(!flags[0] && flags[1]);
}
