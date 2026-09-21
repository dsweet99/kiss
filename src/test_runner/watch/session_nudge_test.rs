#![cfg(unix)]

use super::super::*;
use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::capture_stdout::capture_stdout;
use crate::test_runner::run_test;
use crate::test_runner::test_mode_fixtures::{git_in, init_git};
use crate::test_runner::watch::control::NudgeRequestMsg;
use crate::test_runner::watch::PathSignature;
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
fn targets_alone_do_not_force_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            targets: vec!["tests/a.py".into()],
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut args = py_dry_args();
    args.lang_filter = None;
    args.invocation = TestInvocation::All;
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
        "PATH must not consume pending file changes"
    );
}

#[test]
fn target_idle_replies_named_fail_footer() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            targets: vec!["tests/slow/ops/test_argus.py".into()],
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
        try_reply_idle_nudge(&mut queued, &last, false),
        "named FAIL PATH must idle"
    );
    let msg = wait.recv().unwrap();
    let out = msg.output.unwrap_or_default();
    assert_eq!(msg.exit_code, 1, "FAIL slice exit; out={out:?}");
    assert!(
        out.contains("test_argus_subscribe_counts_published_pings")
            && !out.contains("test_observability")
            && !out.contains("test_ops_eval_measurement_model")
            && out.contains("1 failed"),
        "FAIL PATH slice; out={out:?}"
    );
    assert!(queued.is_none());
}

#[test]
fn lang_filter_alone_does_not_force_new_cycle() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("rust".into()),
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut args = py_dry_args();
    args.lang_filter = None;
    args.invocation = TestInvocation::All;
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
        "--lang must not consume pending file changes"
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
        machine.has_pending_work(),
        "--lang must leave settle pending"
    );
}

#[test]
fn coalesce_lang_then_bare_idle_replies_each_slice() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_lang, w_lang) = mpsc::sync_channel(1);
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("rust".into()),
            ..Default::default()
        },
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
        try_reply_idle_nudge(&mut queued, &last, false),
        "idle --lang and bare must both recap from cache"
    );
    let lang_out = w_lang.recv().unwrap().output.unwrap_or_default();
    let bare_out = w_bare.recv().unwrap().output.unwrap_or_default();
    assert!(
        lang_out.contains("src/lib.rs::t_slow") && !lang_out.contains("tests/a.py::test_a"),
        "--lang rust waiter must get the rust slice; lang={lang_out:?}"
    );
    assert!(
        bare_out.contains("tests/a.py::test_a") && bare_out.contains("src/lib.rs::t_slow"),
        "bare waiter must get the full-suite recap; bare={bare_out:?}"
    );
}

#[test]
fn coalesce_lang_then_bare_pending_keeps_separate_cycles() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_lang, _w_lang) = mpsc::sync_channel(1);
    let (r_bare, _w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("rust".into()),
            ..Default::default()
        },
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
    assert!(machine.has_pending_work(), "--lang head must leave pending");
    let mut args = py_dry_args();
    args.lang_filter = None;
    args.invocation = TestInvocation::All;
    let mut live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    apply_queued_filters(&mut live, &queued);
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(cycle.lang_filter, Some(kiss::Language::Rust));
    let rest = queued.as_ref().expect("bare remains");
    assert!(rest.lang_filter.is_none() && !rest.is_target_scoped());
}

#[test]
fn coalesce_target_then_bare_idle_keeps_full_suite() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_tgt, w_tgt) = mpsc::sync_channel(1);
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            targets: vec!["tests/a.py::test_a".into()],
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
        queued.as_ref().is_some_and(|q| !q.targets.is_empty() && q.next.is_some()),
        "bare must not merge onto TARGET"
    );
    let mut args = py_dry_args();
    args.invocation = TestInvocation::All;
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
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    assert!(w_tgt.try_recv().is_err(), "TARGET waiter stays on the cycle");
    let bare_out = w_bare.recv().unwrap().output.unwrap_or_default();
    assert!(
        bare_out.contains("tests/b.py::test_b") && bare_out.contains("tests/a.py::test_a"),
        "bare waiter must idle the full suite; bare={bare_out:?}"
    );
}

#[test]
fn coalesce_bare_then_target_idle_replies_named_slice() {
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
            targets: vec!["tests/a.py::test_a".into()],
            ..Default::default()
        },
        reply: r_tgt,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued
            .as_ref()
            .is_some_and(|q| q.targets.is_empty() && q.next.is_some()),
        "TARGET must not merge onto bare"
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
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    let bare_out = w_bare.recv().unwrap().output.unwrap_or_default();
    let tgt_out = w_tgt.recv().unwrap().output.unwrap_or_default();
    assert!(
        bare_out.contains("tests/b.py::test_b") && bare_out.contains("tests/a.py::test_a"),
        "bare waiter must idle the full suite; bare={bare_out:?}"
    );
    assert!(
        tgt_out.contains("tests/a.py::test_a") && !tgt_out.contains("tests/b.py::test_b"),
        "TARGET waiter must idle the named slice; tgt={tgt_out:?}"
    );
    assert!(queued.is_none(), "named TARGET must not start a cycle");
}

#[test]
fn coalesce_bare_then_collapsed_target_starts_target_cycle() {
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
            targets: vec!["tests/fast/common/test_sdatetime_types.py".into()],
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
            output: Some(
                "PASS (cached): 11808 selectors\nFAIL tests/b.py::test_b\n".into(),
            ),
            ..Default::default()
        },
    );
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    let bare_out = w_bare.recv().unwrap().output.unwrap_or_default();
    assert!(
        bare_out.contains("11808 selectors") && bare_out.contains("tests/b.py::test_b"),
        "bare waiter must idle the full suite; bare={bare_out:?}"
    );
    assert!(
        w_tgt.try_recv().is_err(),
        "collapsed PASS TARGET must start a cycle"
    );
    let mut args = py_dry_args();
    args.invocation = TestInvocation::All;
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    let (cycle, tgt_replies) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/fast/common/test_sdatetime_types.py".into()])
    );
    assert_eq!(tgt_replies.len(), 1);
    assert!(queued.is_none());
}

#[test]
fn collapsed_pass_target_idles_from_workspace_selectors() {
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
            targets: vec!["tests/fast/common/test_sdatetime_types.py".into()],
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
        try_reply_idle_nudge(&mut queued, &last, false),
        "collapsed PASS PATH must idle from workspace selectors"
    );
    let msg = wait.recv().unwrap();
    let out = msg.output.unwrap_or_default();
    assert_eq!(msg.exit_code, 0, "PASS slice exit; out={out:?}");
    assert!(
        out.contains("test_sdatetime_types.py::test_a")
            && out.contains("test_sdatetime_types.py::test_b")
            && !out.contains("tests/b.py::test_b")
            && out.contains("2 passed"),
        "collapsed PASS PATH slice; out={out:?}"
    );
    assert!(queued.is_none());
}

#[test]
fn coalesce_lang_rust_then_python_idle_replies_each_slice() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_rs, w_rs) = mpsc::sync_channel(1);
    let (r_py, w_py) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("rust".into()),
            ..Default::default()
        },
        reply: r_rs,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("python".into()),
            ..Default::default()
        },
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
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    let rust_out = w_rs.recv().unwrap().output.unwrap_or_default();
    let py_out = w_py.recv().unwrap().output.unwrap_or_default();
    assert!(
        rust_out.contains("src/lib.rs::t_slow") && !rust_out.contains("tests/b.py::test_b"),
        "rust waiter must get the rust slice; rust={rust_out:?}"
    );
    assert!(
        py_out.contains("tests/b.py::test_b") && !py_out.contains("src/lib.rs::t_slow"),
        "python waiter must get the python slice; py={py_out:?}"
    );
}

#[test]
fn coalesce_lang_rust_then_python_pending_keeps_separate_cycles() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_rs, _w_rs) = mpsc::sync_channel(1);
    let (r_py, _w_py) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("rust".into()),
            ..Default::default()
        },
        reply: r_rs,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            lang: Some("python".into()),
            ..Default::default()
        },
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
    assert!(machine.has_pending_work(), "rust head must leave pending");
    let mut args = py_dry_args();
    args.lang_filter = None;
    args.invocation = TestInvocation::All;
    let mut live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    apply_queued_filters(&mut live, &queued);
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(cycle.lang_filter, Some(kiss::Language::Rust));
    let rest = queued.as_ref().expect("python remains");
    assert!(
        rest.lang_filter == Some(kiss::Language::Python) && rest.is_target_scoped()
    );
    force_ready_if_pending(&queued, &mut machine, Path::new("."));
    assert!(
        machine.has_pending_work(),
        "python head must leave pending"
    );
}

#[test]
fn coalesce_retry_bad_then_bare_idle_keeps_full_suite() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (r_bad, w_bad) = mpsc::sync_channel(1);
    let (r_bare, w_bare) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force_bad: true,
            targets: vec!["tests/a.py::test_a".into()],
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
    args.invocation = TestInvocation::All;
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
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    assert!(w_bad.try_recv().is_err(), "retry-bad waiter stays on the cycle");
    let bare_out = w_bare.recv().unwrap().output.unwrap_or_default();
    assert!(
        bare_out.contains("tests/b.py::test_b") && bare_out.contains("tests/a.py::test_a"),
        "bare waiter must idle the full suite; bare={bare_out:?}"
    );
}

#[test]
fn coalesce_bare_then_retry_bad_idle_starts_retry_bad_cycle() {
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
            targets: vec!["tests/a.py::test_a".into()],
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
    assert!(try_reply_idle_nudge(&mut queued, &last, false));
    let bare_out = w_bare.recv().unwrap().output.unwrap_or_default();
    assert!(
        bare_out.contains("tests/b.py::test_b") && bare_out.contains("tests/a.py::test_a"),
        "bare waiter must idle the full suite; bare={bare_out:?}"
    );
    assert!(w_bad.try_recv().is_err(), "retry-bad waiter must not idle");
    let mut args = py_dry_args();
    args.invocation = TestInvocation::All;
    let live = live_from_args_disabled(args, Duration::from_secs(1), Path::new("."));
    let (cycle, bad_replies) = take_queued_cycle_args(&live, &mut queued);
    assert!(cycle.force_bad);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/a.py::test_a".into()])
    );
    assert_eq!(bad_replies.len(), 1);
    assert!(queued.is_none());
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
        base.invocation = invocation.clone();
        let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
        let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
        assert!(cycle.force_rerun, "invocation={invocation:?}");
        assert_eq!(cycle.invocation, invocation);
    }
}

#[test]
fn forwarded_force_two_path_descriptors_override_watcher_modes() {
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
                targets: force_targets.clone(),
                ..Default::default()
            },
            reply,
        })
        .unwrap();
        let mut queued = None;
        coalesce_nudges(Some(&rx), &mut queued);
        let mut base = py_dry_args();
        base.invocation = watcher.clone();
        let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
        let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
        assert!(cycle.force_rerun, "watcher={watcher:?}");
        assert_eq!(
            cycle.invocation,
            TestInvocation::Targets(force_targets.clone()),
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
                0
            );
        });
        let cycles_run = Arc::clone(&cycles);
        let seen_run = Arc::clone(&seen);
        let mut src = NudgeScript {
            steps: timeout_steps(4),
        };
        let mut args = py_dry_args();
        args.invocation = invocation.clone();
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
    use crate::test_runner::watch::control::{NudgeInvocation, NudgeRequestMsg as Msg};
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            invocation: NudgeInvocation::Commit,
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    assert!(
        queued.as_ref().is_some_and(|q| !q.invocation.is_all()),
        "commit must request a new cycle"
    );
    let mut base = py_dry_args();
    base.invocation = TestInvocation::All;
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let (cycle, _) = take_queued_cycle_args(&live, &mut queued);
    assert_eq!(
        cycle.invocation,
        TestInvocation::Commit,
        "commit must run through the shared processor, not the watcher's All recap"
    );
}

#[test]
fn retry_bad_targets_override_watcher_all_invocation() {
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let selected = "tests/fast/app_server/test_transact_grid.py::test_assign_method_follows_grid_queue_after_rebind";
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: false,
            force_bad: true,
            targets: vec![selected.into()],
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.invocation = TestInvocation::All;
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
    use crate::test_runner::watch::control::NudgeRequestMsg as Msg;
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply, _wait) = mpsc::sync_channel(1);
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,
            targets: vec![
                "tests/fast/analysis/test_mmfv_latency_ab_timings_gantt.py::test_gantt_helpers_prepare_assign_and_filter".into(),
            ],
            ..Default::default()
        },
        reply,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.invocation = TestInvocation::All;
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
    base.invocation = TestInvocation::All;
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
            targets: vec!["tests/a.py::test_a".into()],
            ..Default::default()
        },
        reply: r1,
    })
    .unwrap();
    tx.send(NudgeRequest {
        msg: Msg {
            force: true,
            targets: vec!["tests/b.py::test_b".into()],
            ..Default::default()
        },
        reply: r2,
    })
    .unwrap();
    let mut queued = None;
    coalesce_nudges(Some(&rx), &mut queued);
    let mut base = py_dry_args();
    base.invocation = TestInvocation::All;
    let live = live_from_args_disabled(base, Duration::from_secs(1), Path::new("."));
    let q = queued.as_ref().expect("coalesced");
    assert_eq!(
        q.targets,
        vec![
            "tests/a.py::test_a".to_string(),
            "tests/b.py::test_b".to_string()
        ]
    );
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
                force: false,
                force_bad: true,
                targets: vec![selected.into()],
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
    args.invocation = TestInvocation::All;
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
                targets: vec![selected.into()],
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
    args.invocation = TestInvocation::All;
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
                targets: selected_nudge,
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
    args.invocation = TestInvocation::All;
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
        1,
        "idle nudge with no file events must not run another coverage cycle"
    );
}

#[test]
fn idle_target_nudge_slices_named_pass_without_new_cycle() {
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
                targets: vec!["tests/a.py::test_a".into()],
                ..Default::default()
            },
            reply: reply_tx,
        })
        .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let seen_run = Arc::clone(&seen);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
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
        vec![TestInvocation::All],
        "named TARGET must idle from the cached recap"
    );
    let out = reply.output.clone().unwrap_or_default();
    assert!(
        out.contains("1 passed") && !out.contains("2 passed"),
        "targeted waiter must see this cycle, not the merged suite; out={out:?}"
    );
    assert!(
        !out.contains("tests/b.py::test_b"),
        "targeted recap must not list sibling selectors; out={out:?}"
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
                targets: vec!["tests/a.py::test_a".into()],
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
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
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
        targeted_out.contains("1 passed") && !targeted_out.contains("2 passed"),
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
                lang: Some("rust".into()),
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
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
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
        rust_out.contains("src/lib.rs::a_ok") && !rust_out.contains("tests/a.py::test_a"),
        "--lang rust must recap rust only; rust={rust_out:?}"
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
                lang: Some("rust".into()),
                ..Default::default()
            },
            reply: reply_first_tx,
        })
        .unwrap();
        let first = reply_first_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                lang: Some("rust".into()),
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
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
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
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (first, second) = sender.join().unwrap();
    assert_eq!(code, 1);
    let first_out = first.output.clone().unwrap_or_default();
    let second_out = second.output.clone().unwrap_or_default();
    assert!(
        first_out.contains("src/lib.rs::a_ok") && !first_out.contains("tests/a.py::test_a"),
        "first --lang rust={first_out:?}"
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
                lang: Some("rust".into()),
                ..Default::default()
            },
            reply: reply_rust_tx,
        })
        .unwrap();
        reply_rust_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
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
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let rust = sender.join().unwrap();
    assert_eq!(code, 1);
    let rust_out = rust.output.clone().unwrap_or_default();
    assert!(
        rust_out.contains("2753") && !rust_out.contains("8634"),
        "first --lang rust after collapsed bilingual must recap rust only; rust={rust_out:?}"
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
                lang: Some("python".into()),
                ..Default::default()
            },
            reply: reply_first_tx,
        })
        .unwrap();
        let first = reply_first_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                lang: Some("python".into()),
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
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
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
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (first, second) = sender.join().unwrap();
    assert_eq!(code, 1);
    let first_out = first.output.clone().unwrap_or_default();
    let second_out = second.output.clone().unwrap_or_default();
    assert!(
        first_out.contains("8634") && !first_out.contains("773") && !first_out.contains("2753"),
        "first --lang python after split collapsed groups must recap 8634; first={first_out:?}"
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
                lang: Some("rust".into()),
                force: true,
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
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
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
                kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed rust pass 2753");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                return RunTestOnceOutcome::Code(0);
            }
            kiss::rust_llvm_cov_runner::emit_progress("FAIL: src/lib.rs::a_ok (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass",
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
                lang: Some("rust".into()),
                force: true,
                ..Default::default()
            },
            reply: reply_rust_tx,
        })
        .unwrap();
        let rust = reply_rust_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        tx.send(NudgeRequest {
            msg: NudgeRequestMsg {
                lang: Some("python".into()),
                ..Default::default()
            },
            reply: reply_py_tx,
        })
        .unwrap();
        let py = reply_py_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (rust, py)
    });

    let tests_run = std::sync::Arc::clone(&tests);
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
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
                kiss::rust_llvm_cov_runner::emit_progress("kiss test: lang_collapsed rust pass 2753");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass",
                );
                return RunTestOnceOutcome::Code(0);
            }
            kiss::rust_llvm_cov_runner::emit_progress("FAIL: src/lib.rs::a_ok (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let (_rust, py) = sender.join().unwrap();
    assert_eq!(code, 1);
    let py_out = py.output.clone().unwrap_or_default();
    assert_eq!(
        py.exit_code, 0,
        "--lang python after rust fail must keep python exit 0; py={py_out:?} exit={}",
        py.exit_code
    );
    assert!(
        py_out.contains("8634") && !py_out.contains("src/lib.rs::a_ok"),
        "python idle must recap python only; py={py_out:?}"
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
    let mut steps = VecDeque::new();
    steps.push_back(Err(RecvTimeout::Timeout));
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(16));
    let mut src = NudgeScript { steps };
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
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
                RunTestOnceOutcome::Code(1)
            } else {
                kiss::rust_llvm_cov_runner::emit_progress(
                    "PASS (cached): tests/slow/test_ops_hogneato_sim_tuner_smoke_rust.py::test_ops_hogneato_sim_tuner_smoke_rust (0.46s)",
                );
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 1 passed · 0 failed · 0 timed out · 0.46s total · 0s max pass",
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
        out.contains("1 failed")
            && (out.contains("2 passed") || out.contains("3 passed"))
            && out.contains("tests/c.py::test_c"),
        "idle oneshot must recap all known results, not the last cycle only; out={out:?}"
    );
    assert_ne!(
        reply.exit_code, 0,
        "suite still has a failure; reply={reply:?}"
    );
}

fn spawn_nudge_during_barrier(
    entered: std::sync::Arc<std::sync::Barrier>,
    release: std::sync::Arc<std::sync::Barrier>,
    msg: NudgeRequestMsg,
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
            0
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
