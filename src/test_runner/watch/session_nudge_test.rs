#![cfg(unix)]

use super::super::*;
use super::{
    NudgeScript, commit_a_py, publish_pass_count, publish_rows_for_request, publish_workspace_rows,
    py_dry_args, timeout_steps,
};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::capture_stdout::capture_stdout;
use crate::test_runner::run_test;
use crate::test_runner::target_request::EffectiveStatus;
use crate::test_runner::test_mode_fixtures::{git_in, init_git};
use crate::test_runner::watch::PathSignature;
use crate::test_runner::watch::control::NudgeRequestMsg;
use crate::test_runner::watch::event_source::{NormalizedWatchEvent, RecvTimeout};
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

fn send_nudge_after(
    delay: Duration,
    msg: NudgeRequestMsg,
) -> (mpsc::Receiver<NudgeRequest>, std::thread::JoinHandle<()>) {
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
            ..Default::default()
        },
    );
    let mut steps = VecDeque::new();
    steps.push_back(Err(RecvTimeout::Timeout));
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(1));
    let mut src = NudgeScript { steps };
    let t0 = Instant::now();
    let code = run_watch_loop(
        py_dry_args(),
        Duration::from_millis(100),
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
            ..Default::default()
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
    let replies_len;
    {
        let (cycle1, replies) = take_queued_cycle_args(&live, &mut queued);
        assert!(queued.is_none(), "queue consumed");
        assert!(cycle1.force_rerun && cycle1.force_bad && cycle1.metrics);
        replies_len = replies.len();
    }
    assert_eq!(replies_len, 2);
    let (cycle2, replies2) = take_queued_cycle_args(&live, &mut queued);
    assert!(!cycle2.force_rerun && !cycle2.force_bad && !cycle2.metrics);
    assert!(replies2.is_empty());
}

#[test]
fn forwarded_extra_overrides_watcher_and_starts_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, _reply_rx) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            extra: vec!["-k".into(), "does_not_match".into()],
            python_extra: vec!["-k".into(), "does_not_match".into()],
            ..Default::default()
        },
        reply: reply_tx,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let base = py_dry_args();
    let mut live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    queued
        .as_mut()
        .expect("queued")
        .stamp_filter_override(&live);
    assert!(queued.as_ref().expect("queued").wants_new_cycle());
    apply_queued_filters(&mut live, &queued);
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(
        cycle.extra,
        &["-k".to_string(), "does_not_match".to_string()]
    );
    assert_eq!(
        cycle.python_extra,
        &["-k".to_string(), "does_not_match".to_string()]
    );
}

#[test]
fn python_extra_alone_starts_a_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, _reply_rx) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            python_extra: vec!["-k".into(), "does_not_match".into()],
            ..Default::default()
        },
        reply: reply_tx,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let live = live_from_args_disabled(py_dry_args(), Duration::from_secs(1), Path::new("."));
    queued
        .as_mut()
        .expect("queued")
        .stamp_filter_override(&live);
    assert!(queued.as_ref().expect("queued").wants_new_cycle());
}

#[test]
fn targets_alone_do_not_force_new_cycle() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: operands_request(&["tests/a.py".into()], None, &[]),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut args = py_dry_args();
    args.set_lang_filter(None);
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    queued
        .as_mut()
        .expect("queued")
        .stamp_filter_override(&live);
    assert!(
        !queued.as_ref().expect("queued").wants_new_cycle(),
        "PATH must look up a cached recap when files have not changed"
    );
    assert!(
        queued.as_ref().expect("queued").is_target_scoped(),
        "PATH is a scoped identity"
    );
}

#[test]
fn workspace_compat_targets_do_not_withhold_pending() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        !queued.as_ref().expect("queued").is_target_scoped(),
        "Workspace pin plus compat targets is not pin-scoped"
    );
    let mut machine = SettleMachine::new(Duration::from_secs(30));
    machine.note_path(
        PathBuf::from("a.py"),
        Instant::now(),
        PathSignature {
            exists: true,
            modified: None,
            length: 1,
        },
    );
    assert!(machine.has_pending_work());
    force_ready_if_pending(&queued, &mut machine, Path::new("."));
    assert!(
        !machine.has_pending_work(),
        "Workspace compat targets must not leave settle pending"
    );
}

#[test]
fn target_idle_replies_full_fail_recap() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 124,
            output: Some(
                "PASS (cached): 11808 selectors\n\
                 FAIL (cached): 2 selectors\n\
                 TIMEOUT (cached): 1 selectors\n\
                 FAIL tests/slow/ops/test_argus.py::test_argus_subscribe_counts_published_pings\n\
                 TIMEOUT tests/slow/ops/test_observability.py::test_observability\n\
                 FAIL tests/slow/ops/test_ops.py::test_ops_eval_measurement_model\n"
                    .into(),
            ),
            ..Default::default()
        },
    );
    assert!(
        !try_reply_idle_nudge(&mut queued, &last, false),
        "PATH must not idle by slicing last-reply"
    );
    assert!(wait.try_recv().is_err());
    assert!(queued.is_some());
}

#[test]
fn lang_filter_alone_does_not_force_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: crate::test_runner::target_request::request_from_focus(
                crate::test_runner::target_request::TargetFocus::Git(
                    crate::test_runner::target_request::GitFocus::Commit,
                ),
                Some(kiss::Language::Rust),
                &[],
            ),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut args = py_dry_args();
    args.set_lang_filter(None);
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    queued
        .as_mut()
        .expect("queued")
        .stamp_filter_override(&live);
    assert!(
        !queued.as_ref().expect("queued").wants_new_cycle(),
        "--lang must look up a cached recap when files have not changed"
    );
    assert!(
        queued.as_ref().expect("queued").is_target_scoped(),
        "--lang remains a language identity"
    );
    let mut machine = SettleMachine::new(Duration::from_secs(30));
    let now = Instant::now();
    machine.note_path(
        PathBuf::from("a.py"),
        now,
        PathSignature {
            exists: true,
            modified: None,
            length: 1,
        },
    );
    assert!(machine.has_pending_work());
    force_ready_if_pending(&queued, &mut machine, Path::new("."));
    assert!(
        !machine.has_pending_work(),
        "--lang is a normal query and must force settle"
    );
}

#[test]
fn metrics_alone_do_not_force_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            metrics: true,
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut args = py_dry_args();
    args.set_lang_filter(None);
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    queued
        .as_mut()
        .expect("queued")
        .stamp_filter_override(&live);
    assert!(
        !queued.as_ref().expect("queued").wants_new_cycle(),
        "metrics-only must look up a cached recap when files have not changed"
    );
    assert!(
        queued.as_ref().expect("queued").metrics,
        "metrics remains output-only on the queued cycle"
    );
}

#[test]
fn coalesce_lang_then_bare_idle_replies_each_slice() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_lang, w_lang) = mpsc::sync_channel(1);
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default().with_lang_label("rust"),
        reply: r_lang,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 124,
            output: Some("PASS: tests/a.py::test_a\nTIMEOUT: src/lib.rs::t_slow\n".into()),
            ..Default::default()
        },
    );
    last.store(
        Some(kiss::Language::Rust),
        NudgeReplyMsg {
            exit_code: 124,
            output: Some("TIMEOUT: src/lib.rs::t_slow\n".into()),
            ..Default::default()
        },
    );
    assert!(
        !try_reply_idle_nudge(&mut queued, &last, false),
        "idle --lang and bare without ready TargetReport must start a cycle"
    );
    assert!(w_lang.try_recv().is_err());
    assert!(w_bare.try_recv().is_err());
}

#[test]
fn coalesce_lang_then_bare_pending_keeps_separate_cycles() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_lang, _w_lang) = mpsc::sync_channel(1);
    let (r_bare, _w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default().with_lang_label("rust"),
        reply: r_lang,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert!(
        q.lang_filter == Some(kiss::Language::Rust) && q.next.is_some(),
        "bare must not merge onto --lang"
    );
    assert!(q.is_target_scoped());
    let mut machine = SettleMachine::new(Duration::from_secs(30));
    machine.note_path(
        PathBuf::from("a.py"),
        Instant::now(),
        PathSignature {
            exists: true,
            modified: None,
            length: 1,
        },
    );
    force_ready_if_pending(&queued, &mut machine, Path::new("."));
    assert!(
        !machine.has_pending_work(),
        "--lang head is a normal query and must force settle"
    );
    let mut args = py_dry_args();
    args.set_lang_filter(None);
    args.set_invocation(TestInvocation::All);
    let mut live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    apply_queued_filters(&mut live, &queued);
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(cycle.lang_filter, Some(kiss::Language::Rust));
    let rest = queued.as_ref().expect("bare remains");
    assert!(rest.lang_filter.is_none() && !rest.is_target_scoped());
}

#[test]
fn coalesce_force_request_does_not_union_other_path() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_a, _w_a) = mpsc::sync_channel(1);
    let (r_b, _w_b) = mpsc::sync_channel(1);
    let request = operands_request(&["tests/a.py".into()], None, &[]);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,

            target_request: request,
            ..Default::default()
        },
        reply: r_a,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,

            ..Default::default()
        },
        reply: r_b,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert!(
        q.next.is_none(),
        "Workspace force without a typed operand pin is unscoped and merges"
    );
    assert!(q.unscoped_force);
    assert!(
        q.targets.is_empty(),
        "unscoped force must not copy protocol targets: {:?}",
        q.targets
    );
}

#[test]
fn coalesce_different_path_targets_without_request_union() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_a, _w_a) = mpsc::sync_channel(1);
    let (r_b, _w_b) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply: r_a,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply: r_b,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert!(q.next.is_none(), "same Workspace pin must merge");
    assert!(
        q.targets.is_empty(),
        "Workspace pin must not copy protocol targets: {:?}",
        q.targets
    );
}

#[test]
fn coalesce_different_operand_requests_stay_separate() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_a, _w_a) = mpsc::sync_channel(1);
    let (r_b, _w_b) = mpsc::sync_channel(1);
    let first = operands_request(&["tests/a.py".into()], None, &[]);
    let second = operands_request(&["tests/b.py".into()], None, &[]);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: first,
            ..Default::default()
        },
        reply: r_a,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: second,
            ..Default::default()
        },
        reply: r_b,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert!(
        q.next.is_some(),
        "different operand TargetRequests must not merge"
    );
    assert_eq!(q.targets, vec!["tests/a.py".to_string()]);
    assert_eq!(
        q.next.as_ref().map(|n| n.targets.clone()),
        Some(vec!["tests/b.py".to_string()])
    );
}

#[test]
fn coalesce_unsorted_operands_request_sorts_targets() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: operands_request(
                &["tests/z.py".into(), "tests/a.py".into()],
                None,
                &[],
            ),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert_eq!(
        q.targets,
        vec!["tests/a.py".to_string(), "tests/z.py".to_string()]
    );
    assert!(!q.targets.iter().any(|t| t == "stale.py"));
}

#[test]
fn can_merge_operand_pin_ignores_compat_targets() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r1, _w1) = mpsc::sync_channel(1);
    let (r2, _w2) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: operands_request(&["tests/a.py".into()], None, &[]),
            ..Default::default()
        },
        reply: r1,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: operands_request(&["tests/a.py".into()], None, &[]),
            ..Default::default()
        },
        reply: r2,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert!(q.next.is_none(), "same operand pin must merge");
    assert_eq!(q.targets, vec!["tests/a.py".to_string()]);
    assert!(!q.targets.iter().any(|t| t == "stale.py" || t == "other.py"));
}

#[test]
fn coalesce_target_then_bare_idle_keeps_full_suite() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_tgt, w_tgt) = mpsc::sync_channel(1);
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: operands_request(&["tests/a.py::test_a".into()], None, &[]),
            ..Default::default()
        },
        reply: r_tgt,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued
            .as_ref()
            .is_some_and(|q| !q.targets.is_empty() && q.next.is_some()),
        "bare must not merge onto TARGET"
    );
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    let (cycle, tgt_replies) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/a.py::test_a".into()])
    );
    assert_eq!(tgt_replies.len(), 1);
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 1,
            output: Some("PASS: tests/a.py::test_a\nFAIL: tests/b.py::test_b\n".into()),
            ..Default::default()
        },
    );
    assert!(!try_reply_idle_nudge(&mut queued, &last, false));
    assert!(
        w_tgt.try_recv().is_err(),
        "TARGET waiter stays on the cycle"
    );
    assert!(
        w_bare.try_recv().is_err(),
        "bare must not idle from last-reply transcript"
    );
}

#[test]
fn coalesce_bare_then_target_idle_replies_full_recap() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    let (r_tgt, w_tgt) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: crate::test_runner::target_request::operands_request(
                &["tests/a.py::test_a".into()],
                None,
                &[],
            ),
            ..Default::default()
        },
        reply: r_tgt,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued.as_ref().is_some_and(|q| q.next.is_some()),
        "named TARGET must not merge onto bare"
    );
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 1,
            output: Some("PASS: tests/a.py::test_a\nFAIL: tests/b.py::test_b\n".into()),
            ..Default::default()
        },
    );
    assert!(!try_reply_idle_nudge(&mut queued, &last, false));
    assert!(
        w_bare.try_recv().is_err(),
        "bare must not idle from last-reply transcript"
    );
    assert!(
        w_tgt.try_recv().is_err(),
        "TARGET must not idle by slicing last-reply"
    );
    assert!(queued.is_some(), "named TARGET must start a cycle");
}

#[test]
fn coalesce_bare_then_collapsed_target_starts_target_cycle() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    let (r_tgt, w_tgt) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: operands_request(
                &["tests/fast/common/test_sdatetime_types.py".into()],
                None,
                &[],
            ),
            ..Default::default()
        },
        reply: r_tgt,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 1,
            output: Some("PASS (cached): 11808 selectors\nFAIL tests/b.py::test_b\n".into()),
            ..Default::default()
        },
    );
    assert!(
        !try_reply_idle_nudge(&mut queued, &last, false),
        "workspace head without a ready TargetReport must start a cycle"
    );
    assert!(w_bare.try_recv().is_err());
    assert!(
        w_tgt.try_recv().is_err(),
        "collapsed PASS TARGET must start a cycle"
    );
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    let (cycle, head_replies) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(cycle.invocation, TestInvocation::All);
    assert_eq!(head_replies.len(), 1);
    assert!(
        queued.is_some(),
        "TARGET stays queued behind the workspace head"
    );
}

#[test]
fn collapsed_pass_target_idles_full_recap() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    use crate::test_runner::workspace_selector_cache::store_python_workspace_selectors;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("tests/fast/common")).unwrap();
    std::fs::write(
        root.join("tests/fast/common/test_sdatetime_types.py"),
        "def test_a():\n    assert True\n\ndef test_b():\n    assert True\n",
    )
    .unwrap();
    assert!(store_python_workspace_selectors(
        root,
        &[],
        &[
            "tests/fast/common/test_sdatetime_types.py::test_a".into(),
            "tests/fast/common/test_sdatetime_types.py::test_b".into(),
        ],
        &[],
    ));
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut last = LastReplies::for_repo(root);
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 1,
            output: Some(
                "PASS (cached): 11808 selectors\n\
                 FAIL (cached): 1 selectors\n\
                 FAIL tests/b.py::test_b\n"
                    .into(),
            ),
            ..Default::default()
        },
    );
    assert!(
        !try_reply_idle_nudge(&mut queued, &last, false),
        "PATH must not idle by expanding last-reply"
    );
    assert!(wait.try_recv().is_err());
    assert!(queued.is_some());
}

#[test]
fn coalesce_lang_rust_then_python_idle_replies_each_slice() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_rs, w_rs) = mpsc::sync_channel(1);
    let (r_py, w_py) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default().with_lang_label("rust"),
        reply: r_rs,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default().with_lang_label("python"),
        reply: r_py,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued
            .as_ref()
            .is_some_and(|q| q.lang_filter == Some(kiss::Language::Rust) && q.next.is_some()),
        "--lang python must not merge onto --lang rust"
    );
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        Some(kiss::Language::Rust),
        NudgeReplyMsg {
            exit_code: 124,
            output: Some("TIMEOUT: src/lib.rs::t_slow\n".into()),
            ..Default::default()
        },
    );
    last.store(
        Some(kiss::Language::Python),
        NudgeReplyMsg {
            exit_code: 1,
            output: Some("FAIL: tests/b.py::test_b\n".into()),
            ..Default::default()
        },
    );
    assert!(!try_reply_idle_nudge(&mut queued, &last, false));
    assert!(w_rs.try_recv().is_err());
    assert!(w_py.try_recv().is_err());
}

#[test]
fn coalesce_lang_rust_then_python_pending_keeps_separate_cycles() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_rs, _w_rs) = mpsc::sync_channel(1);
    let (r_py, _w_py) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default().with_lang_label("rust"),
        reply: r_rs,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default().with_lang_label("python"),
        reply: r_py,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.as_ref().expect("queued");
    assert!(
        q.lang_filter == Some(kiss::Language::Rust) && q.next.is_some(),
        "--lang python must not merge onto --lang rust"
    );
    assert!(q.is_target_scoped());
    let mut machine = SettleMachine::new(Duration::from_secs(30));
    machine.note_path(
        PathBuf::from("a.py"),
        Instant::now(),
        PathSignature {
            exists: true,
            modified: None,
            length: 1,
        },
    );
    force_ready_if_pending(&queued, &mut machine, Path::new("."));
    assert!(
        !machine.has_pending_work(),
        "rust head is a normal query and must force settle"
    );
    let mut args = py_dry_args();
    args.set_lang_filter(None);
    args.set_invocation(TestInvocation::All);
    let mut live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    apply_queued_filters(&mut live, &queued);
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(cycle.lang_filter, Some(kiss::Language::Rust));
    let rest = queued.as_ref().expect("python remains");
    assert!(rest.lang_filter == Some(kiss::Language::Python) && rest.is_target_scoped());
    force_ready_if_pending(&queued, &mut machine, Path::new("."));
    assert!(
        !machine.has_pending_work(),
        "python head is a normal query and must force settle"
    );
}

#[test]
fn coalesce_retry_bad_then_bare_idle_keeps_full_suite() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_bad, w_bad) = mpsc::sync_channel(1);
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force_bad: true,

            target_request: operands_request(&["tests/a.py::test_a".into()], None, &[]),
            ..Default::default()
        },
        reply: r_bad,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued
            .as_ref()
            .is_some_and(|q| q.force_bad && !q.targets.is_empty() && q.next.is_some()),
        "bare must not merge onto --retry-bad TARGET"
    );
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    let (cycle, bad_replies) = take_queued_cycle_args(&live, &mut queued);
    assert!(cycle.force_bad);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/a.py::test_a".into()])
    );
    assert_eq!(bad_replies.len(), 1);
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 1,
            output: Some("PASS: tests/a.py::test_a\nFAIL: tests/b.py::test_b\n".into()),
            ..Default::default()
        },
    );
    assert!(!try_reply_idle_nudge(&mut queued, &last, false));
    assert!(
        w_bad.try_recv().is_err(),
        "retry-bad waiter stays on the cycle"
    );
    assert!(
        w_bare.try_recv().is_err(),
        "bare must not idle from last-reply transcript"
    );
}

#[test]
fn coalesce_bare_then_retry_bad_idle_starts_retry_bad_cycle() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    let (r_bad, w_bad) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg::default(),
        reply: r_bare,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            force_bad: true,

            target_request: operands_request(&["tests/a.py::test_a".into()], None, &[]),
            ..Default::default()
        },
        reply: r_bad,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued
            .as_ref()
            .is_some_and(|q| q.targets.is_empty() && !q.force_bad && q.next.is_some()),
        "--retry-bad TARGET must not merge onto bare"
    );
    let mut last = LastReplies::for_repo(Path::new("."));
    last.store(
        None,
        NudgeReplyMsg {
            exit_code: 1,
            output: Some("PASS: tests/a.py::test_a\nFAIL: tests/b.py::test_b\n".into()),
            ..Default::default()
        },
    );
    assert!(!try_reply_idle_nudge(&mut queued, &last, false));
    assert!(w_bare.try_recv().is_err());
    assert!(w_bad.try_recv().is_err(), "retry-bad waiter must not idle");
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    let (cycle, head_replies) = take_queued_cycle_args(&live, &mut queued);
    assert!(!cycle.force_bad);
    assert_eq!(cycle.invocation, TestInvocation::All);
    assert_eq!(head_replies.len(), 1);
    assert!(
        queued.as_ref().is_some_and(|q| q.force_bad),
        "retry-bad stays queued behind the workspace head"
    );
}

fn watcher_running_invocations() -> Vec<TestInvocation> {
    vec![
        TestInvocation::Commit,
        TestInvocation::Base,
        TestInvocation::Main,
        TestInvocation::Targets(vec!["pkg".into()]),
        TestInvocation::Targets(vec!["tests".into()]),
        TestInvocation::Targets(vec!["tests/test_app.py".into()]),
        TestInvocation::Targets(vec!["tests/test_app.py::test_value".into()]),
        TestInvocation::Targets(vec!["pkg/models.py::Group".into()]),
        TestInvocation::Targets(vec!["pkg/models.py::Group.__init__".into()]),
        TestInvocation::Targets(vec!["tests/test_group.py::TestUser.test_email".into()]),
        TestInvocation::Targets(vec!["tests/test_group.py::TestUser::test_email".into()]),
        TestInvocation::Targets(vec!["tests/test_params.py::test_item[0]".into()]),
        TestInvocation::Targets(vec!["src/lib.rs".into()]),
        TestInvocation::Targets(vec!["src/lib.rs::value".into()]),
        TestInvocation::Targets(vec!["tests/smoke.rs".into()]),
        TestInvocation::Targets(vec!["src/lib.rs::gets_value".into()]),
        TestInvocation::Targets(vec!["src/lib.rs".into(), "tests/test_app.py".into()]),
    ]
}

#[test]
fn unscoped_force_keeps_watcher_commit_base_main_and_path_descriptors() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    for invocation in watcher_running_invocations() {
        let (tx, rx) = mpsc::channel::<NudgeRequest>();
        let (reply, _wait) = mpsc::sync_channel(1);
        tx.send(NudgeRequest {
            msg: Msg {
                force: true,
                ..Default::default()
            },
            reply,
        })
        .unwrap();
        let mut queued = None;
        coalesce_nudges(Some(&rx), &mut queued);
        let mut base = py_dry_args();
        base.set_invocation(invocation.clone());
        let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
        let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
        assert!(cycle.force_rerun, "invocation={invocation:?}");
        assert_eq!(cycle.invocation, invocation);
    }
}

#[test]
fn forwarded_force_two_path_descriptors_override_watcher_modes() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let force_targets = vec![
        "tests/test_app.py::test_value".to_string(),
        "src/lib.rs::gets_value".to_string(),
    ];
    for watcher in [
        TestInvocation::All,
        TestInvocation::Commit,
        TestInvocation::Base,
        TestInvocation::Main,
        TestInvocation::Targets(vec!["pkg".into()]),
    ] {
        let (tx, rx) = mpsc::channel::<NudgeRequest>();
        let (reply, _wait) = mpsc::sync_channel(1);
        tx.send(NudgeRequest {
            msg: Msg {
                force: true,

                target_request: operands_request(&force_targets, None, &[]),
                ..Default::default()
            },
            reply,
        })
        .unwrap();
        let mut queued = None;
        coalesce_nudges(Some(&rx), &mut queued);
        let mut base = py_dry_args();
        base.set_invocation(watcher.clone());
        let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
        let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
        assert!(cycle.force_rerun, "watcher={watcher:?}");
        let mut expected = force_targets.clone();
        expected.sort();
        assert_eq!(
            cycle.invocation,
            TestInvocation::Targets(expected),
            "watcher={watcher:?}"
        );
    }
}

#[test]
fn unscoped_force_nudge_keeps_running_watcher_invocation() {
    // Coalesce coverage of every descriptor lives in
    // `unscoped_force_keeps_watcher_commit_base_main_and_path_descriptors`.
    // This live-loop check only needs a few representative running invocations.
    let representative = [
        TestInvocation::Commit,
        TestInvocation::Targets(vec!["tests/test_app.py::test_value".into()]),
        TestInvocation::Targets(vec!["src/lib.rs".into(), "tests/test_app.py".into()]),
    ];
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    for invocation in representative {
        let (tx, rx) = mpsc::channel::<NudgeRequest>();
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let cycles = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::<(bool, TestInvocation)>::new()));
        let cycles_nudge = Arc::clone(&cycles);
        let sender = std::thread::spawn(move || {
            while cycles_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
                std::thread::sleep(Duration::from_millis(1));
            }
            std::thread::sleep(Duration::from_millis(5));
            tx.send(NudgeRequest {
                msg: NudgeRequestMsg {
                    force: true,
                    ..Default::default()
                },
                reply: reply_tx,
            })
            .unwrap();
            assert_eq!(
                reply_rx
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .exit_code,
                1
            );
        });
        let cycles_run = Arc::clone(&cycles);
        let seen_run = Arc::clone(&seen);
        let mut src = NudgeScript {
            steps: timeout_steps(4),
        };
        let mut args = py_dry_args();
        args.set_invocation(invocation.clone());
        let code = run_watch_loop_with(
            args,
            Duration::from_secs(3600),
            tmp.path(),
            &mut src,
            Some(&rx),
            move |cycle_args| {
                cycles_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                seen_run
                    .lock()
                    .unwrap()
                    .push((cycle_args.force_rerun, cycle_args.invocation.clone()));
                RunTestOnceOutcome::Code(0)
            },
            |_args| WatchCoverageResult::ok(0),
        );
        sender.join().unwrap();
        assert_eq!(code, 1, "invocation={invocation:?}");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen.len(),
            2,
            "initial plus unscoped force; invocation={invocation:?}"
        );
        assert!(!seen[0].0 && seen[0].1 == invocation, "initial={seen:?}");
        assert!(seen[1].0 && seen[1].1 == invocation, "forced={seen:?}");
    }
}

#[test]
fn commit_nudge_overrides_watcher_all_invocation() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: crate::test_runner::target_request::request_from_focus(
                crate::test_runner::target_request::TargetFocus::Git(
                    crate::test_runner::target_request::GitFocus::Commit,
                ),
                None,
                &[],
            ),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued.as_ref().is_some_and(|q| matches!(
            q.target_request.focus,
            crate::test_runner::target_request::TargetFocus::Git(
                crate::test_runner::target_request::GitFocus::Commit
            )
        )),
        "commit must keep Git Commit identity"
    );
    let mut base = py_dry_args();
    base.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Commit,
        "commit must run through the shared processor, not the watcher's All recap"
    );
}

#[test]
fn queued_target_request_commit_is_git_commit() {
    use crate::test_runner::target_request::{GitFocus, TargetFocus};
    let (reply, _wait) = mpsc::sync_channel(1);
    let q = QueuedCycle {
        replies: vec![(None, reply)],
        force: false,
        force_bad: false,
        metrics: false,
        targets: Vec::new(),
        unscoped_force: false,
        lang_filter: None,
        ignore: Vec::new(),
        extra: Vec::new(),
        python_extra: Vec::new(),
        filter_override: false,
        coverage_all: false,
        target_request: crate::test_runner::target_request::request_from_focus(
            TargetFocus::Git(GitFocus::Commit),
            None,
            &[],
        ),
        runner: String::new(),
        configuration: String::new(),
        next: None,
    };
    assert!(matches!(
        super::super::queued_target_request(&q).focus,
        TargetFocus::Git(GitFocus::Commit)
    ));
}

#[test]
fn queued_all_without_request_builds_workspace_request() {
    use crate::test_runner::target_request::{is_workspace_focus, workspace_request};
    let (reply, _wait) = mpsc::sync_channel(1);
    let q = QueuedCycle {
        replies: vec![(None, reply)],
        force: false,
        force_bad: false,
        metrics: false,
        targets: Vec::new(),
        unscoped_force: false,
        lang_filter: None,
        ignore: Vec::new(),
        extra: Vec::new(),
        python_extra: Vec::new(),
        filter_override: false,
        coverage_all: false,
        target_request: crate::test_runner::target_request::workspace_request(None, &[]),
        runner: String::new(),
        configuration: String::new(),
        next: None,
    };
    let request = super::super::queued_target_request(&q);
    assert!(is_workspace_focus(&request.focus));
    assert_eq!(request, workspace_request(None, &[]));
}

#[test]
fn queued_target_request_clones_operand_pin_sorted() {
    use crate::test_runner::target_request::{operand_raws, operands_request};
    let (reply, _wait) = mpsc::sync_channel(1);
    let q = QueuedCycle {
        replies: vec![(None, reply)],
        force: false,
        force_bad: false,
        metrics: false,
        targets: Vec::new(),
        unscoped_force: false,
        lang_filter: None,
        ignore: Vec::new(),
        extra: Vec::new(),
        python_extra: Vec::new(),
        filter_override: false,
        coverage_all: false,
        target_request: operands_request(&["z.py".into(), "a.py".into()], None, &[]),
        runner: String::new(),
        configuration: String::new(),
        next: None,
    };
    assert_eq!(
        operand_raws(&super::super::queued_target_request(&q).focus),
        Some(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn from_req_clones_operand_pin_sorted() {
    use crate::test_runner::target_request::operands_request;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: NudgeRequestMsg {
            target_request: operands_request(&["z.py".into(), "a.py".into()], None, &[]),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.expect("operand pin should queue");
    assert!(matches!(
        q.target_request.focus,
        crate::test_runner::target_request::TargetFocus::Operands(_)
    ));
    assert_eq!(q.targets, vec!["a.py".to_string(), "z.py".to_string()]);
}

#[test]
fn pin_from_nudge_msg_clones_operand_pin_sorted() {
    use crate::test_runner::target_request::{operand_raws, operands_request};
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: NudgeRequestMsg {
            target_request: operands_request(&["z.py".into(), "a.py".into()], None, &[]),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.expect("operand pin should queue");
    assert_eq!(
        operand_raws(&q.target_request.focus),
        Some(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn pin_from_nudge_msg_clones_workspace_pin_ignores_compat_lang() {
    use crate::test_runner::target_request::workspace_request;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: NudgeRequestMsg {
            target_request: workspace_request(None, &[]),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let q = queued.expect("workspace pin should queue");
    assert_eq!(q.target_request, workspace_request(None, &[]));
    assert_eq!(q.lang_filter, None);
}

#[test]
fn queued_workspace_focus_prefers_target_request() {
    use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
    let (reply, _wait) = mpsc::sync_channel(1);
    let mut q = QueuedCycle {
        replies: vec![(None, reply)],
        force: false,
        force_bad: false,
        metrics: false,
        targets: Vec::new(),
        unscoped_force: false,
        lang_filter: None,
        ignore: Vec::new(),
        extra: Vec::new(),
        python_extra: Vec::new(),
        filter_override: false,
        coverage_all: false,
        target_request: request_from_focus(TargetFocus::Git(GitFocus::Commit), None, &[]),
        runner: String::new(),
        configuration: String::new(),
        next: None,
    };
    assert!(!q.is_workspace_focus());
    assert!(q.is_target_scoped());
    q.target_request = crate::test_runner::target_request::workspace_request(None, &[]);
    assert!(q.is_workspace_focus());
    assert!(!q.is_target_scoped());
    q.targets.push("tests/a.py".into());
    assert!(q.is_workspace_focus());
    assert!(
        !q.is_target_scoped(),
        "Workspace compat targets must not withhold pending files"
    );
}

#[test]
fn queued_all_without_request_is_workspace_focus() {
    let (reply, _wait) = mpsc::sync_channel(1);
    let q = QueuedCycle {
        replies: vec![(None, reply)],
        force: false,
        force_bad: false,
        metrics: false,
        targets: Vec::new(),
        unscoped_force: false,
        lang_filter: None,
        ignore: Vec::new(),
        extra: Vec::new(),
        python_extra: Vec::new(),
        filter_override: false,
        coverage_all: false,
        target_request: crate::test_runner::target_request::workspace_request(None, &[]),
        runner: String::new(),
        configuration: String::new(),
        next: None,
    };
    assert!(q.is_workspace_focus());
    assert!(!q.is_target_scoped());
}

#[test]
fn retry_bad_targets_override_watcher_all_invocation() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let selected = "tests/fast/app_server/test_transact_grid.py::test_assign_method_follows_grid_queue_after_rebind";
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: false,
            force_bad: true,

            target_request: operands_request(&[selected.into()], None, &[]),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert!(cycle.force_bad);
    assert!(!cycle.force_rerun);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec![selected.into()]),
        "retry-bad TARGET must scope the watcher cycle"
    );
}

#[test]
fn forwarded_force_targets_override_watcher_all_invocation() {
    use crate::test_runner::target_request::operands_request;
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,

            target_request: operands_request(
                &["tests/fast/analysis/test_mmfv_latency_ab_timings_gantt.py::test_gantt_helpers_prepare_assign_and_filter".into()],
                None,
                &[],
            ),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert!(cycle.force_rerun);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec![
            "tests/fast/analysis/test_mmfv_latency_ab_timings_gantt.py::test_gantt_helpers_prepare_assign_and_filter".into()
        ])
    );
}

#[test]
fn commit_nudge_does_not_want_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        !queued.as_ref().expect("queued").wants_new_cycle(),
        "commit is a normal query; force/retry/filter own a new cycle"
    );
}

#[test]
fn commit_without_target_report_does_not_idle_on_workspace_last_reply() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut last = LastReplies::for_repo(tmp.path());
    last.store(
        Some(kiss::Language::Rust),
        NudgeReplyMsg {
            exit_code: 0,
            output: Some("✓ 3 passed · 0 failed · 0 timed out\n".into()),
            ..Default::default()
        },
    );
    assert!(
        !try_reply_idle_nudge(&mut queued, &last, false),
        "commit must not replay the workspace last-reply"
    );
    assert!(wait.try_recv().is_err());
    assert!(queued.is_some());
}

#[test]
fn main_idle_recaps_ready_target_report() {
    use crate::test_runner::target_request::{
        EnsurePolicy, GitFocus, TargetFocus, load_ready_for_request, materialize_target_report,
        request_from_focus,
    };
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    use crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors;
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
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
    let request = request_from_focus(
        TargetFocus::Git(GitFocus::DefaultMain),
        Some(kiss::Language::Rust),
        &[],
    );
    let report = materialize_target_report(
        tmp.path(),
        &request,
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    crate::test_runner::target_request::publish_if_rows_hold(tmp.path(), &request, &report)
        .unwrap();
    assert!(
        load_ready_for_request(tmp.path(), &request, false, &[]).is_some(),
        "seeded main report must load as ready"
    );
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: request.clone(),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let last = LastReplies::for_repo(tmp.path());
    assert!(
        try_reply_idle_nudge(&mut queued, &last, false),
        "ready main TargetReport must idle"
    );
    let out = wait.recv().unwrap().output.unwrap_or_default();
    assert!(out.contains("passed"), "main idle official text; out={out}");
    assert!(queued.is_none());
}

#[test]
fn main_idle_misses_ready_target_report_when_runner_token_differs() {
    use crate::test_runner::target_request::{
        EnsurePolicy, GitFocus, TargetFocus, load_ready_for_request, materialize_target_report,
        request_from_focus,
    };
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    use crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors;
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
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
    let request = request_from_focus(
        TargetFocus::Git(GitFocus::DefaultMain),
        Some(kiss::Language::Rust),
        &[],
    );
    let report = materialize_target_report(
        tmp.path(),
        &request,
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    crate::test_runner::target_request::publish_if_rows_hold(tmp.path(), &request, &report)
        .unwrap();
    assert!(
        load_ready_for_request(tmp.path(), &request, false, &[]).is_some(),
        "seeded main report must load as ready"
    );
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            runner: "other-runner".into(),
            target_request: crate::test_runner::target_request::request_from_focus(
                crate::test_runner::target_request::TargetFocus::Git(
                    crate::test_runner::target_request::GitFocus::DefaultMain,
                ),
                Some(kiss::Language::Rust),
                &[],
            ),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let last = LastReplies::for_repo(tmp.path());
    assert!(
        !try_reply_idle_nudge(&mut queued, &last, false),
        "runner-token mismatch must not idle"
    );
    assert!(wait.try_recv().is_err());
    assert!(queued.is_some());
}

#[test]
fn commit_idle_assembles_complete_cached_target_report() {
    use crate::test_runner::target_request::{
        EnsurePolicy, GitFocus, TargetFocus, load_ready_for_request, materialize_target_report,
        request_from_focus,
    };
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    use crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors;
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
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
    fs::write(tmp.path().join("app.py"), "x = 2\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "app.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "change"])
            .status()
            .unwrap()
            .success()
    );
    assert!(store_rust_workspace_selectors(tmp.path(), &[], &[]));
    let request = request_from_focus(
        TargetFocus::Git(GitFocus::Commit),
        Some(kiss::Language::Rust),
        &[],
    );
    let report = materialize_target_report(
        tmp.path(),
        &request,
        &EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    assert!(
        load_ready_for_request(tmp.path(), &request, false, &[]).is_none(),
        "the query must assemble complete evidence that was not stored"
    );
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            target_request: request.clone(),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let last = LastReplies::for_repo(tmp.path());
    assert!(
        try_reply_idle_nudge(&mut queued, &last, false),
        "complete cached all TargetReport must idle when last-reply is absent"
    );
    let out = wait.recv().unwrap().output.unwrap_or_default();
    assert_eq!(
        out,
        crate::test_runner::target_request::official_report_text(&report),
        "idle commit reply must equal the typed TargetReport rendering"
    );
    assert!(queued.is_none());
}

#[test]
fn unscoped_force_keeps_watcher_all_invocation() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert!(cycle.force_rerun);
    assert_eq!(cycle.invocation, TestInvocation::All);
}

#[test]
fn coalesce_unions_force_targets_until_unscoped_force() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r1, _w1) = mpsc::sync_channel(1);
    let (r2, _w2) = mpsc::sync_channel(1);
    let (r3, _w3) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,

            ..Default::default()
        },
        reply: r1,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,

            ..Default::default()
        },
        reply: r2,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.set_invocation(TestInvocation::All);
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let q = queued.as_ref().expect("coalesced");
    assert!(
        q.targets.is_empty(),
        "Workspace force pins must not union protocol targets: {:?}",
        q.targets
    );
    assert!(q.unscoped_force);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,
            ..Default::default()
        },
        reply: r3,
    })
    .unwrap();
    coalesce_nudges(Some(&rx), &mut queued);
    let (cycle, replies) = take_queued_cycle_args(&live, &mut queued);
    assert!(cycle.force_rerun);
    assert_eq!(cycle.invocation, TestInvocation::All);
    assert_eq!(replies.len(), 3);
}

#[test]
fn retry_bad_nudge_reruns_only_selected_fake_python_test_on_tmp_repo() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    fs::write(
        tmp.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         test_coverage_threshold = 0\n\
         orphan_detection = false\n",
    )
    .unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_pair.py"),
        "def test_first():\n    assert True\n\ndef test_second():\n    assert True\n",
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
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            tmp.path(),
            &[],
            &[
                "tests/test_pair.py::test_first".into(),
                "tests/test_pair.py::test_second".into(),
            ],
            &[],
        )
    );

    let selected = "tests/test_pair.py::test_first";
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let cycles = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::<TestInvocation>::new()));
    let forced_out = Arc::new(Mutex::new(String::new()));
    let cycles_nudge = Arc::clone(&cycles);
    let sender = std::thread::spawn(move || {
        while cycles_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: false,
                force_bad: true,

                target_request: crate::test_runner::target_request::operands_request(
                    &[selected.into()],
                    None,
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_tx,
        })
        .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(30))
                .unwrap()
                .exit_code,
            0
        );
    });

    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let cycles_run = Arc::clone(&cycles);
    let seen_run = Arc::clone(&seen);
    let out_run = Arc::clone(&forced_out);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = cycles_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            seen_run.lock().unwrap().push(cycle_args.invocation.clone());
            if n == 1 {
                return RunTestOnceOutcome::Code(0);
            }
            let mut code = 1;
            let stdout = capture_stdout(|| {
                code = run_test(cycle_args);
            });
            *out_run.lock().unwrap() = stdout.clone();
            assert_eq!(code, 0, "retry-bad cycle must exit 0; out={stdout}");
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
    let invocations = seen.lock().unwrap().clone();
    assert_eq!(invocations.len(), 2, "initial All plus retry-bad TARGET");
    assert_eq!(invocations[0], TestInvocation::All);
    assert_eq!(
        invocations[1],
        TestInvocation::Targets(vec![selected.to_string()])
    );
    let stdout = forced_out.lock().unwrap().clone();
    assert!(
        stdout.contains("test_pair.py::test_first"),
        "retry-bad selector must run, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("test_pair.py::test_second"),
        "sibling test must not run when TARGET is set, got:\n{stdout}"
    );
}

#[test]
fn targeted_force_nudge_reruns_only_selected_fake_python_test() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_pair.py"),
        "def test_first():\n    assert True\n\ndef test_second():\n    assert True\n",
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
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );

    let selected = "tests/test_pair.py::test_first";
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let cycles = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::<TestInvocation>::new()));
    let forced_out = Arc::new(Mutex::new(String::new()));
    let cycles_nudge = Arc::clone(&cycles);
    let sender = std::thread::spawn(move || {
        while cycles_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,

                target_request: crate::test_runner::target_request::operands_request(
                    &[selected.into()],
                    None,
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_tx,
        })
        .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(30))
                .unwrap()
                .exit_code,
            0
        );
    });

    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let cycles_run = Arc::clone(&cycles);
    let seen_run = Arc::clone(&seen);
    let out_run = Arc::clone(&forced_out);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = cycles_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            seen_run.lock().unwrap().push(cycle_args.invocation.clone());
            if n == 1 {
                return RunTestOnceOutcome::Code(0);
            }
            let stdout = capture_stdout(|| {
                assert_eq!(run_test(cycle_args), 0);
            });
            *out_run.lock().unwrap() = stdout;
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
    let invocations = seen.lock().unwrap().clone();
    assert_eq!(invocations.len(), 2, "initial All plus targeted force");
    assert_eq!(invocations[0], TestInvocation::All);
    assert_eq!(
        invocations[1],
        TestInvocation::Targets(vec![selected.to_string()])
    );
    let stdout = forced_out.lock().unwrap().clone();
    assert!(
        stdout.contains("test_pair.py::test_first"),
        "forced selector must run, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("test_pair.py::test_second"),
        "sibling test must not run, got:\n{stdout}"
    );
}

#[test]
fn targeted_force_nudge_reruns_two_selected_fake_python_tests() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_trio.py"),
        "def test_first():\n    assert True\n\ndef test_second():\n    assert True\n\ndef test_third():\n    assert True\n",
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
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );

    let selected = vec![
        "tests/test_trio.py::test_first".to_string(),
        "tests/test_trio.py::test_third".to_string(),
    ];
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let cycles = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::<TestInvocation>::new()));
    let forced_out = Arc::new(Mutex::new(String::new()));
    let cycles_nudge = Arc::clone(&cycles);
    let selected_nudge = selected.clone();
    let sender = std::thread::spawn(move || {
        while cycles_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,

                target_request: crate::test_runner::target_request::operands_request(
                    &selected_nudge,
                    None,
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_tx,
        })
        .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(30))
                .unwrap()
                .exit_code,
            0
        );
    });

    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let cycles_run = Arc::clone(&cycles);
    let seen_run = Arc::clone(&seen);
    let out_run = Arc::clone(&forced_out);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let mut args = py_dry_args();
    args.set_invocation(TestInvocation::All);
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = cycles_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            seen_run.lock().unwrap().push(cycle_args.invocation.clone());
            if n == 1 {
                return RunTestOnceOutcome::Code(0);
            }
            let stdout = capture_stdout(|| {
                assert_eq!(run_test(cycle_args), 0);
            });
            *out_run.lock().unwrap() = stdout;
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
    let invocations = seen.lock().unwrap().clone();
    assert_eq!(invocations.len(), 2, "initial All plus targeted force");
    assert_eq!(invocations[0], TestInvocation::All);
    assert_eq!(invocations[1], TestInvocation::Targets(selected.clone()));
    let stdout = forced_out.lock().unwrap().clone();
    assert!(
        stdout.contains("test_trio.py::test_first"),
        "first forced selector must run, got:\n{stdout}"
    );
    assert!(
        stdout.contains("test_trio.py::test_third"),
        "second forced selector must run, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("test_trio.py::test_second"),
        "unselected sibling must not run, got:\n{stdout}"
    );
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
            publish_pass_count(tmp.path(), 1);
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        cycles.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "overlapping idle nudges must reuse the last completed cycle"
    );
}

#[test]
fn idle_nudge_without_file_events_must_not_start_another_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let covs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
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
    let covs_run = std::sync::Arc::clone(&covs);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        |_args| {
            tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            publish_workspace_rows(
                tmp.path(),
                &[("python", "tests/a.py::test_a", EffectiveStatus::Pass)],
                0,
            );
            RunTestOnceOutcome::Code(0)
        },
        move |_args| {
            covs_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            WatchCoverageResult::ok(0)
        },
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "idle nudge with no file events must not run another test cycle"
    );
    assert_eq!(
        covs.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "retired run_cov must not run on the first cycle or the idle nudge"
    );
}

#[test]
fn idle_target_nudge_replies_full_recap_without_new_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::<TestInvocation>::new()));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                target_request: crate::test_runner::target_request::operands_request(
                    &["tests/a.py::test_a".into()],
                    None,
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_tx,
        })
        .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let seen_run = Arc::clone(&seen);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            seen_run.lock().unwrap().push(cycle_args.invocation.clone());
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/b.py::test_b (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 2 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                publish_workspace_rows(
                    &repo,
                    &[
                        ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                        ("python", "tests/b.py::test_b", EffectiveStatus::Pass),
                    ],
                    0,
                );
            } else {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
                );
            }
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let reply = sender.join().unwrap();
    assert_eq!(code, 1);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            TestInvocation::All,
            TestInvocation::Targets(vec!["tests/a.py::test_a".into()]),
        ],
        "named TARGET without TargetReport must start a cycle"
    );
    let out = reply.output.clone().unwrap_or_default();
    assert!(
        out.contains("passed") || out.contains("report members=") || out.is_empty(),
        "targeted official body is TargetReport or typed miss; out={out:?}"
    );
}

#[test]
fn unscoped_idle_nudge_after_target_still_recaps_full_suite() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_target_tx, reply_target_rx) = mpsc::sync_channel(1);
    let (reply_idle_tx, reply_idle_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_target_tx,
        })
        .unwrap();
        let targeted = reply_target_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_idle_tx,
        })
        .unwrap();
        let idle = reply_idle_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (targeted, idle)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/b.py::test_b (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 2 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                publish_workspace_rows(
                    &repo,
                    &[
                        ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                        ("python", "tests/b.py::test_b", EffectiveStatus::Pass),
                    ],
                    0,
                );
            } else {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
                );
            }
            let _ = cycle_args;
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (targeted, idle) = sender.join().unwrap();
    assert_eq!(code, 1);
    let targeted_out = targeted.output.clone().unwrap_or_default();
    let idle_out = idle.output.clone().unwrap_or_default();
    assert!(
        targeted_out.contains("2 passed") && targeted_out.contains("tests/b.py::test_b"),
        "TARGET waiter={targeted_out:?}"
    );
    assert!(
        idle_out.contains("2 passed") && idle_out.contains("tests/b.py::test_b"),
        "unscoped idle recap must keep the last full cycle; idle={idle_out:?}"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "named TARGET and the later bare nudge must both idle"
    );
}

#[test]
fn unscoped_idle_nudge_after_lang_rust_recaps_full_suite() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_rust_tx, reply_rust_rx) = mpsc::sync_channel(1);
    let (reply_idle_tx, reply_idle_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_rust_tx,
        })
        .unwrap();
        let rust = reply_rust_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_idle_tx,
        })
        .unwrap();
        let idle = reply_idle_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (rust, idle)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress("PASS: src/lib.rs::a_ok (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 2 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                publish_workspace_rows(
                    &repo,
                    &[
                        ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                        ("rust", "src/lib.rs::a_ok", EffectiveStatus::Pass),
                    ],
                    0,
                );
            } else {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: src/lib.rs::a_ok (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
                );
            }
            let _ = cycle_args;
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (rust, idle) = sender.join().unwrap();
    assert_eq!(code, 1);
    let rust_out = rust.output.clone().unwrap_or_default();
    let idle_out = idle.output.clone().unwrap_or_default();
    assert!(
        rust_out.contains("src/lib.rs::a_ok") && rust_out.contains("tests/a.py::test_a"),
        "--lang rust without a rust TargetRequest idles the ready workspace report; rust={rust_out:?}"
    );
    assert!(
        idle_out.contains("2 passed")
            && idle_out.contains("tests/a.py::test_a")
            && idle_out.contains("src/lib.rs::a_ok"),
        "bare kiss test after --lang rust must recap both languages; idle={idle_out:?}"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "cached --lang rust and bare idle must not start another cycle"
    );
}

#[test]
fn lang_rust_nudge_reuses_cached_rust_recap() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_first_tx, reply_first_rx) = mpsc::sync_channel(1);
    let (reply_second_tx, reply_second_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_first_tx,
        })
        .unwrap();
        let first = reply_first_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_second_tx,
        })
        .unwrap();
        let second = reply_second_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        (first, second)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress("PASS: src/lib.rs::a_ok (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✓ 2 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
            );
            publish_workspace_rows(
                &repo,
                &[
                    ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                    ("rust", "src/lib.rs::a_ok", EffectiveStatus::Pass),
                ],
                0,
            );
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (first, second) = sender.join().unwrap();
    assert_eq!(code, 1);
    let first_out = first.output.clone().unwrap_or_default();
    let second_out = second.output.clone().unwrap_or_default();
    assert!(
        first_out.contains("src/lib.rs::a_ok") && first_out.contains("tests/a.py::test_a"),
        "first --lang rust idles the ready workspace report; first={first_out:?}"
    );
    assert_eq!(
        first_out, second_out,
        "second --lang rust must replay the cached rust recap"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "--lang rust cache hit must not start another cycle"
    );
}

#[test]
fn collapsed_bilingual_lang_rust_idles_without_new_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_rust_tx, reply_rust_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_rust_tx,
        })
        .unwrap();
        reply_rust_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 8634 selectors");
            kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed python pass 8634");
            kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 2753 selectors");
            kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed rust pass 2753");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
            );
            publish_pass_count(&repo, 1);
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let rust = sender.join().unwrap();
    assert_eq!(code, 1);
    let rust_out = rust.output.clone().unwrap_or_default();
    assert!(
        rust_out.contains("passed") || rust_out.contains("report members="),
        "first --lang rust after collapsed bilingual idles the ready workspace report; rust={rust_out:?}"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "collapsed bilingual --lang rust must idle without another cycle"
    );
}

#[test]
fn collapsed_split_python_lang_python_idles_full_count() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_first_tx, reply_first_rx) = mpsc::sync_channel(1);
    let (reply_second_tx, reply_second_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_first_tx,
        })
        .unwrap();
        let first = reply_first_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                ..Default::default()
            },
            reply: reply_second_tx,
        })
        .unwrap();
        let second = reply_second_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        (first, second)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 7861 selectors");
            kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed python pass 7861");
            kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 773 selectors");
            kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed python pass 773");
            kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 2753 selectors");
            kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed rust pass 2753");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
            );
            publish_pass_count(&repo, 1);
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (first, second) = sender.join().unwrap();
    assert_eq!(code, 1);
    let first_out = first.output.clone().unwrap_or_default();
    let second_out = second.output.clone().unwrap_or_default();
    assert!(
        first_out.contains("passed") || first_out.contains("report members="),
        "first --lang python after split collapsed groups idles the ready workspace report; first={first_out:?}"
    );
    assert_eq!(
        first_out, second_out,
        "second --lang python must replay the full python recap, not 773"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "split python collapsed --lang python must idle without another cycle"
    );
}

#[test]
fn bare_idle_after_forced_lang_rust_fail_recaps_failure() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_rust_tx, reply_rust_rx) = mpsc::sync_channel(1);
    let (reply_idle_tx, reply_idle_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Rust),
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_rust_tx,
        })
        .unwrap();
        let rust = reply_rust_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_idle_tx,
        })
        .unwrap();
        let idle = reply_idle_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (rust, idle)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 8634 selectors");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "kiss test: lang_collapsed python pass 8634",
                );
                kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 2753 selectors");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "kiss test: lang_collapsed rust pass 2753",
                );
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                return RunTestOnceOutcome::Code(0);
            }
            kiss::rust_llvm_cov_runner::emit_progress("FAIL: src/lib.rs::a_ok (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            publish_workspace_rows(
                &repo,
                &[("rust", "src/lib.rs::a_ok", EffectiveStatus::Fail)],
                1,
            );
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (rust, idle) = sender.join().unwrap();
    assert_eq!(code, 1);
    let rust_out = rust.output.clone().unwrap_or_default();
    let idle_out = idle.output.clone().unwrap_or_default();
    assert!(
        rust_out.contains("src/lib.rs::a_ok") && rust_out.contains("failed"),
        "--lang rust fail={rust_out:?}"
    );
    assert!(
        idle.exit_code != 0 && idle_out.contains("failed") && idle_out.contains("src/lib.rs::a_ok"),
        "bare kiss test after failing --lang rust must recap the failure; idle={idle_out:?}"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "force --lang rust runs once after the bilingual cycle"
    );
}

#[test]
fn lang_python_idle_after_rust_fail_keeps_python_exit_zero() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_rust_tx, reply_rust_rx) = mpsc::sync_channel(1);
    let (reply_py_tx, reply_py_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                force: true,
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Rust),
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_rust_tx,
        })
        .unwrap();
        let rust = reply_rust_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Python),
                    &[],
                ),
                ..Default::default()
            },
            reply: reply_py_tx,
        })
        .unwrap();
        let py = reply_py_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (rust, py)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let code = run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 8634 selectors");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "kiss test: lang_collapsed python pass 8634",
                );
                kiss::rust_llvm_cov_runner::emit_progress("PASS (cached): 2753 selectors");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "kiss test: lang_collapsed rust pass 2753",
                );
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                publish_rows_for_request(
                    &repo,
                    &crate::test_runner::target_request::workspace_request(
                        Some(kiss::Language::Python),
                        &[],
                    ),
                    &[("python", "tests/a.py::test_a", EffectiveStatus::Pass)],
                    0,
                );
                return RunTestOnceOutcome::Code(0);
            }
            kiss::rust_llvm_cov_runner::emit_progress("FAIL: src/lib.rs::a_ok (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            publish_rows_for_request(
                &repo,
                &crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Rust),
                    &[],
                ),
                &[("rust", "src/lib.rs::a_ok", EffectiveStatus::Fail)],
                1,
            );
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (_rust, py) = sender.join().unwrap();
    assert_eq!(code, 1);
    let py_out = py.output.clone().unwrap_or_default();
    assert!(
        !py_out.contains("src/lib.rs::a_ok"),
        "python identity must not recap a rust fail report; py={py_out:?}"
    );
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "--lang python must idle after the forced rust fail"
    );
}

#[test]
fn idle_nudge_recaps_all_known_pass_fail_timeouts() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let file = commit_a_py(&tmp);

    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let tests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let tests_nudge = std::sync::Arc::clone(&tests);
    let sender = std::thread::spawn(move || {
        while tests_nudge.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg::default(),
            reply: reply_tx,
        })
        .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut steps = VecDeque::new();
    steps.push_back(Err(RecvTimeout::Timeout));
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(16));
    let mut src = NudgeScript { steps };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    let code = run_watch_loop_with(
        args,
        Duration::from_millis(1),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            let n = tests_run.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/b.py::test_b (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress("FAIL: tests/c.py::test_c (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✗ 2 passed · 1 failed · 0 timed out · 1s total · 0s max pass",
                );
                kiss::rust_llvm_cov_runner::emit_progress("FAIL tests/c.py::test_c");
                publish_workspace_rows(
                    &repo,
                    &[
                        ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                        ("python", "tests/b.py::test_b", EffectiveStatus::Pass),
                        ("python", "tests/c.py::test_c", EffectiveStatus::Fail),
                    ],
                    1,
                );
                RunTestOnceOutcome::Code(1)
            } else {
                kiss::rust_llvm_cov_runner::emit_progress(
                    "PASS (cached): tests/slow/test_ops_hogneato_sim_tuner_smoke_rust.py::test_ops_hogneato_sim_tuner_smoke_rust (0.46s)",
                );
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 1 passed · 0 failed · 0 timed out · 0.46s total · 0s max pass",
                );
                publish_workspace_rows(
                    &repo,
                    &[(
                        "python",
                        "tests/slow/test_ops_hogneato_sim_tuner_smoke_rust.py::test_ops_hogneato_sim_tuner_smoke_rust",
                        EffectiveStatus::Pass,
                    )],
                    0,
                );
                RunTestOnceOutcome::Code(0)
            }
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let reply = sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        tests.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "file change runs a second cycle; idle nudge must not start a third"
    );
    let out = reply.output.clone().unwrap_or_default();
    assert!(
        out.contains("1 passed")
            && out.contains("tests/slow/test_ops_hogneato_sim_tuner_smoke_rust.py::test_ops_hogneato_sim_tuner_smoke_rust"),
        "idle oneshot recaps the last ready TargetReport; out={out:?}"
    );
    assert_eq!(
        reply.exit_code, 0,
        "last ready TargetReport passed; reply={reply:?}"
    );
}

fn spawn_nudge_during_barrier(
    entered: std::sync::Arc<std::sync::Barrier>,
    release: std::sync::Arc<std::sync::Barrier>,
    msg: NudgeRequestMsg,
    expected_exit: i32,
) -> (mpsc::Receiver<NudgeRequest>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        entered.wait();
        tx.send(NudgeRequest {
            msg,
            reply: reply_tx,
        })
        .unwrap();
        release.wait();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .exit_code,
            expected_exit
        );
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
fn nudge_while_cycle_in_flight_does_not_run_second_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
    let release = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (rx, sender) = spawn_nudge_during_barrier(
        Arc::clone(&entered),
        Arc::clone(&release),
        NudgeRequestMsg::default(),
        0,
    );
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_c = Arc::clone(&seen);
    let entered_c = Arc::clone(&entered);
    let release_c = Arc::clone(&release);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(8),
    };
    let code = run_watch_loop_with(
        py_dry_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |args| {
            let outcome = record_force_and_block_first(&args, &seen_c, &entered_c, &release_c);
            publish_pass_count(&repo, 1);
            outcome
        },
        |_args| WatchCoverageResult::ok(0),
    );
    sender.join().unwrap();
    assert_eq!(code, 1);
    let flags = seen.lock().unwrap().clone();
    assert_eq!(
        flags,
        vec![false],
        "default mid-cycle nudge must not start a second cycle"
    );
}

#[test]
fn nudge_while_cycle_in_flight_runs_second_cycle_before_reply() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
    let release = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (rx, sender) = spawn_nudge_during_barrier(
        Arc::clone(&entered),
        Arc::clone(&release),
        NudgeRequestMsg {
            force: true,
            force_bad: false,
            metrics: false,
            ..Default::default()
        },
        1,
    );
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
