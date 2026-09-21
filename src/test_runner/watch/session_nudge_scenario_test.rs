#![cfg(unix)]

use super::super::*;
use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::last_status::{python_last_status_identity, record_statuses};
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::{NudgeInvocation, NudgeReplyMsg, NudgeRequestMsg};
use crate::test_runner::watch::event_source::NormalizedWatchEvent;
use kiss::rpytest_runner::TestStatus;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn emit_full_suite() {
    kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress("FAIL: tests/b.py::test_b (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress("TIMEOUT: src/lib.rs::t_slow (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress(
        "✗ 1 passed · 1 failed · 1 timed out · 1s total · 0s max pass",
    );
}

fn emit_bilingual_full() {
    kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/b.py::test_b (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress("PASS: src/lib.rs::t_ok (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress("TIMEOUT: src/lib.rs::t_slow (0.01s)");
    kiss::rust_llvm_cov_runner::emit_progress(
        "✗ 3 passed · 0 failed · 1 timed out · 1s total · 0s max pass",
    );
}

fn emit_one_pass(selector: &str) {
    kiss::rust_llvm_cov_runner::emit_progress(&format!("PASS: {selector} (0.01s)"));
    kiss::rust_llvm_cov_runner::emit_progress(
        "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
    );
}

fn nudge_after_cycles(
    tests: Arc<AtomicUsize>,
    ready_at: usize,
    msgs: Vec<NudgeRequestMsg>,
) -> (
    mpsc::Receiver<NudgeRequest>,
    std::thread::JoinHandle<Vec<NudgeReplyMsg>>,
) {
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let sender = std::thread::spawn(move || {
        while tests.load(Ordering::SeqCst) < ready_at {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        let mut replies = Vec::with_capacity(msgs.len());
        for msg in msgs {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(NudgeRequest {
                msg,
                reply: reply_tx,
            })
            .unwrap();
            replies.push(reply_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        replies
    });
    (rx, sender)
}

fn tool_version(code: &str) -> String {
    let out = std::process::Command::new("python")
        .args(["-c", code])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn seed_python_bad(root: &Path, bad: &[(&str, TestStatus)]) {
    let identity = python_last_status_identity(
        &tool_version("import sys; print('.'.join(map(str, sys.version_info[:3])))"),
        &tool_version("import pytest; print(pytest.__version__)"),
        &[],
    );
    let statuses: Vec<(String, TestStatus)> = bad
        .iter()
        .map(|(sel, status)| ((*sel).to_string(), *status))
        .collect();
    record_statuses(root, kiss::Language::Python, &identity, &statuses).unwrap();
}

fn watch_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = py_dry_args();
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = None;
    args
}

fn recap_has_all(out: &str, needles: &[&str]) -> bool {
    needles.iter().all(|n| out.contains(n))
}

fn recap_lacks_all(out: &str, needles: &[&str]) -> bool {
    needles.iter().all(|n| !out.contains(n))
}

fn assert_lang_then_bare(lang: &str, lang_has: &[&str], lang_lacks: &[&str]) {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let tests = Arc::new(AtomicUsize::new(0));
    let (rx, sender) = nudge_after_cycles(
        Arc::clone(&tests),
        1,
        vec![
            NudgeRequestMsg {
                lang: Some(lang.into()),
                ..Default::default()
            },
            NudgeRequestMsg::default(),
        ],
    );
    let tests_run = Arc::clone(&tests);
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let code = run_watch_loop_with(
        watch_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            tests_run.fetch_add(1, Ordering::SeqCst);
            emit_full_suite();
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let replies = sender.join().unwrap();
    assert_eq!(code, 1);
    let lang_out = replies[0].output.clone().unwrap_or_default();
    let idle_out = replies[1].output.clone().unwrap_or_default();
    assert!(
        recap_has_all(&lang_out, lang_has) && recap_lacks_all(&lang_out, lang_lacks),
        "--lang {lang} must recap that language only; out={lang_out:?}"
    );
    assert!(
        recap_has_all(
            &idle_out,
            &[
                "tests/a.py::test_a",
                "tests/b.py::test_b",
                "src/lib.rs::t_slow",
            ]
        ),
        "bare kiss test must recap Python and Rust; idle={idle_out:?}"
    );
    assert_eq!(
        tests.load(Ordering::SeqCst),
        1,
        "cached --lang {lang} and bare idle must not start another cycle"
    );
    assert_ne!(
        replies[0].exit_code, 0,
        "--lang {lang} must use the cached slice exit; out={lang_out:?}"
    );
    assert_ne!(
        replies[1].exit_code, 0,
        "bare kiss test must use the cached full-suite exit; idle={idle_out:?}"
    );
}

#[test]
fn scenario_1_no_files_changed() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let tests = Arc::new(AtomicUsize::new(0));
    let (rx, sender) = nudge_after_cycles(
        Arc::clone(&tests),
        1,
        vec![NudgeRequestMsg::default()],
    );
    let tests_run = Arc::clone(&tests);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let code = run_watch_loop_with(
        watch_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            tests_run.fetch_add(1, Ordering::SeqCst);
            emit_full_suite();
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let replies = sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        tests.load(Ordering::SeqCst),
        1,
        "idle kiss test must not start another cycle"
    );
    let out = replies[0].output.clone().unwrap_or_default();
    assert!(
        out.contains("tests/a.py::test_a")
            && out.contains("tests/b.py::test_b")
            && out.contains("src/lib.rs::t_slow")
            && out.contains("1 failed")
            && out.contains("1 timed out"),
        "idle recap must list Python and Rust PASS/FAIL/TIMEOUT; out={out:?}"
    );
    assert_ne!(replies[0].exit_code, 0, "suite still has FAIL/TIMEOUT");
}

#[test]
fn scenario_2_files_changed() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let file = commit_a_py(&tmp);
    let tests = Arc::new(AtomicUsize::new(0));
    let (rx, sender) = nudge_after_cycles(
        Arc::clone(&tests),
        1,
        vec![NudgeRequestMsg::default(), NudgeRequestMsg::default()],
    );
    let tests_run = Arc::clone(&tests);
    let mut steps = VecDeque::new();
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(12));
    let mut src = NudgeScript { steps };
    let code = run_watch_loop_with(
        watch_args(),
        Duration::from_secs(30),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            let n = tests_run.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                emit_full_suite();
                return RunTestOnceOutcome::Code(1);
            }
            kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let replies = sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(
        tests.load(Ordering::SeqCst),
        2,
        "changed files must start a new cycle"
    );
    let out = replies[0].output.clone().unwrap_or_default();
    assert_eq!(
        replies[0].exit_code, 0,
        "oneshot must use the post-edit cycle exit, not the first-cycle fail; out={out:?}"
    );
    assert!(
        out.contains("tests/a.py::test_a")
            && out.contains("1 passed")
            && !out.contains("1 failed")
            && !out.contains("tests/b.py::test_b")
            && !out.contains("src/lib.rs::t_slow"),
        "oneshot must recap the post-edit cycle, not the first-cycle suite; out={out:?}"
    );
    let later = replies[1].output.clone().unwrap_or_default();
    assert!(
        later.contains("src/lib.rs::t_slow") && later.contains("tests/b.py::test_b"),
        "later idle must keep full-suite TIMEOUT/FAIL siblings; later={later:?}"
    );
    assert_eq!(
        tests.load(Ordering::SeqCst),
        2,
        "later idle after a green incremental must not start a third cycle"
    );
}

#[test]
fn scenario_2_lang_then_bare_after_file_change() {
    for lang in ["rust", "python"] {
        let tmp = tempfile::tempdir().unwrap();
        init_git(&tmp);
        let file = commit_a_py(&tmp);
        let tests = Arc::new(AtomicUsize::new(0));
        let (rx, sender) = nudge_after_cycles(
            Arc::clone(&tests),
            1,
            vec![
                NudgeRequestMsg {
                    lang: Some(lang.into()),
                    ..Default::default()
                },
                NudgeRequestMsg::default(),
            ],
        );
        let tests_run = Arc::clone(&tests);
        let mut steps = VecDeque::new();
        steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
        steps.extend(timeout_steps(16));
        let mut src = NudgeScript { steps };
        let code = run_watch_loop_with(
            watch_args(),
            Duration::from_secs(30),
            tmp.path(),
            &mut src,
            Some(&rx),
            move |_args| {
                let n = tests_run.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    emit_full_suite();
                    return RunTestOnceOutcome::Code(1);
                }
                kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
                );
                RunTestOnceOutcome::Code(0)
            },
            |_args| WatchCoverageResult::ok(0),
        );
        assert_eq!(code, 1, "lang={lang}");
        assert_eq!(
            tests.load(Ordering::SeqCst),
            3,
            "--lang {lang} after a file change must leave pending so bare kiss test starts a third cycle"
        );
        let later = sender.join().unwrap()[1].output.clone().unwrap_or_default();
        assert!(
            later.contains("tests/a.py::test_a"),
            "bare oneshot after --lang {lang} plus pending must recap the unscoped incremental; later={later:?}"
        );
    }
}

#[test]
fn scenario_3_target_while_watcher_on_full_suite() {
    let (replies, cycles) = run_full_then_target_then_idle(
        watch_args(),
        vec![
            NudgeRequestMsg {
                targets: vec!["tests/a.py::test_a".into()],
                ..Default::default()
            },
            NudgeRequestMsg::default(),
        ],
        "tests/a.py::test_a",
    );
    let targeted = replies[0].output.clone().unwrap_or_default();
    let idle = replies[1].output.clone().unwrap_or_default();
    assert!(
        targeted.contains("1 passed")
            && !targeted.contains("2 passed")
            && !targeted.contains("tests/b.py::test_b"),
        "TARGET recap must omit siblings; targeted={targeted:?}"
    );
    assert!(
        idle.contains("tests/b.py::test_b") && idle.contains("src/lib.rs::t_slow"),
        "later bare kiss test must keep the full-suite recap; idle={idle:?}"
    );
    assert_eq!(
        cycles, 1,
        "named TARGET must idle; bare kiss test must not start another cycle"
    );
}

fn run_full_then_target_then_idle(
    watch: crate::test_runner::RunTestCmdArgs<'static>,
    msgs: Vec<NudgeRequestMsg>,
    target: &'static str,
) -> (Vec<NudgeReplyMsg>, usize) {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let tests = Arc::new(AtomicUsize::new(0));
    let (rx, sender) = nudge_after_cycles(Arc::clone(&tests), 1, msgs);
    let tests_run = Arc::clone(&tests);
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let code = run_watch_loop_with(
        watch,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_args| {
            let n = tests_run.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                emit_bilingual_full();
            } else {
                emit_one_pass(target);
            }
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    assert_eq!(code, 1);
    let replies = sender.join().unwrap();
    (replies, tests.load(Ordering::SeqCst))
}

#[test]
fn scenario_3_lang_target_keeps_python_idle_slice() {
    let (replies, cycles) = run_full_then_target_then_idle(
        watch_args(),
        vec![
            NudgeRequestMsg {
                targets: vec!["tests/a.py::test_a".into()],
                lang: Some("python".into()),
                ..Default::default()
            },
            NudgeRequestMsg {
                lang: Some("python".into()),
                ..Default::default()
            },
            NudgeRequestMsg::default(),
        ],
        "tests/a.py::test_a",
    );
    let targeted = replies[0].output.clone().unwrap_or_default();
    let lang_idle = replies[1].output.clone().unwrap_or_default();
    let bare = replies[2].output.clone().unwrap_or_default();
    assert!(
        targeted.contains("1 passed")
            && !targeted.contains("2 passed")
            && !targeted.contains("tests/b.py::test_b"),
        "TARGET recap must omit siblings; targeted={targeted:?}"
    );
    assert!(
        lang_idle.contains("tests/b.py::test_b") && lang_idle.contains("2 passed"),
        "later --lang python must keep the full python recap; lang_idle={lang_idle:?}"
    );
    assert!(
        bare.contains("tests/b.py::test_b") && bare.contains("src/lib.rs::t_slow"),
        "later bare kiss test must keep the full-suite recap; bare={bare:?}"
    );
    assert_eq!(
        cycles, 1,
        "named TARGET must idle; later --lang python and bare must not start extra cycles"
    );
}

#[test]
fn scenario_3_watch_lang_inherits_onto_target_without_clobbering_slice() {
    let mut watch = watch_args();
    watch.lang_filter = Some(kiss::Language::Python);
    let (replies, cycles) = run_full_then_target_then_idle(
        watch,
        vec![
            NudgeRequestMsg {
                targets: vec!["tests/a.py::test_a".into()],
                ..Default::default()
            },
            NudgeRequestMsg {
                lang: Some("python".into()),
                ..Default::default()
            },
        ],
        "tests/a.py::test_a",
    );
    let targeted = replies[0].output.clone().unwrap_or_default();
    let lang_idle = replies[1].output.clone().unwrap_or_default();
    assert!(
        targeted.contains("1 passed")
            && !targeted.contains("2 passed")
            && !targeted.contains("tests/b.py::test_b"),
        "TARGET recap must omit siblings; targeted={targeted:?}"
    );
    assert!(
        lang_idle.contains("tests/b.py::test_b") && lang_idle.contains("2 passed"),
        "later --lang python must keep the full python recap after inherited-lang TARGET; lang_idle={lang_idle:?}"
    );
    assert_eq!(
        cycles, 1,
        "named TARGET must idle; later --lang python must not start another cycle"
    );
}

#[test]
fn scenario_3_lang_target_keeps_rust_idle_slice() {
    let (replies, cycles) = run_full_then_target_then_idle(
        watch_args(),
        vec![
            NudgeRequestMsg {
                targets: vec!["src/lib.rs::t_ok".into()],
                lang: Some("rust".into()),
                ..Default::default()
            },
            NudgeRequestMsg {
                lang: Some("rust".into()),
                ..Default::default()
            },
            NudgeRequestMsg::default(),
        ],
        "src/lib.rs::t_ok",
    );
    let targeted = replies[0].output.clone().unwrap_or_default();
    let lang_idle = replies[1].output.clone().unwrap_or_default();
    let bare = replies[2].output.clone().unwrap_or_default();
    assert!(
        targeted.contains("1 passed")
            && !targeted.contains("src/lib.rs::t_slow")
            && !targeted.contains("tests/b.py::test_b"),
        "TARGET recap must omit rust and python siblings; targeted={targeted:?}"
    );
    assert!(
        lang_idle.contains("src/lib.rs::t_slow") && !lang_idle.contains("tests/b.py::test_b"),
        "later --lang rust must keep the full rust recap; lang_idle={lang_idle:?}"
    );
    assert!(
        bare.contains("src/lib.rs::t_slow") && bare.contains("tests/b.py::test_b"),
        "later bare kiss test must keep the full-suite recap; bare={bare:?}"
    );
    assert_eq!(
        cycles, 1,
        "named TARGET must idle; later --lang rust and bare must not start extra cycles"
    );
}

fn run_scoped_then_file_change(first: NudgeRequestMsg) -> (String, String, String, usize) {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let file = commit_a_py(&tmp);
    let tests = Arc::new(AtomicUsize::new(0));
    let (rx, sender) = nudge_after_cycles(
        Arc::clone(&tests),
        1,
        vec![
            first,
            NudgeRequestMsg::default(),
            NudgeRequestMsg::default(),
        ],
    );
    let tests_run = Arc::clone(&tests);
    let mut steps = VecDeque::new();
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file])]));
    steps.extend(timeout_steps(16));
    let mut src = NudgeScript { steps };
    let code = run_watch_loop_with(
        watch_args(),
        Duration::from_secs(30),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            tests_run.fetch_add(1, Ordering::SeqCst);
            if !matches!(cycle_args.invocation, TestInvocation::All) {
                emit_one_pass("tests/a.py::test_a");
                return RunTestOnceOutcome::Code(0);
            }
            if tests_run.load(Ordering::SeqCst) == 1 {
                emit_bilingual_full();
                return RunTestOnceOutcome::Code(0);
            }
            kiss::rust_llvm_cov_runner::emit_progress("FAIL: tests/a.py::test_a (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    assert_eq!(code, 1);
    let replies = sender.join().unwrap();
    (
        replies[0].output.clone().unwrap_or_default(),
        replies[1].output.clone().unwrap_or_default(),
        replies[2].output.clone().unwrap_or_default(),
        tests.load(Ordering::SeqCst),
    )
}

fn assert_scoped_then_file_change(
    targeted: String,
    after: String,
    later: String,
    cycles: usize,
) {
    assert!(
        targeted.contains("1 passed") && !targeted.contains("tests/b.py::test_b"),
        "scoped recap must omit siblings; targeted={targeted:?}"
    );
    assert!(
        after.contains("FAIL")
            && after.contains("tests/a.py::test_a")
            && !after.contains("1 passed"),
        "oneshot after scoped cycle plus file change must recap the new cycle; after={after:?}"
    );
    assert!(
        later.contains("src/lib.rs::t_slow") && later.contains("tests/b.py::test_b"),
        "later idle must keep full-suite TIMEOUT/PASS siblings; later={later:?}"
    );
    assert_eq!(
        cycles, 3,
        "scoped cycle then file change must run a third cycle"
    );
}

#[test]
fn scenario_3_target_then_file_change_uses_new_cycle() {
    let (targeted, after, later, cycles) = run_scoped_then_file_change(NudgeRequestMsg {
        targets: vec!["tests/a.py::test_a".into()],
        ..Default::default()
    });
    assert_scoped_then_file_change(targeted, after, later, cycles);
}

#[test]
fn scenario_3_commit_base_main_then_file_change_uses_new_cycle() {
    for invocation in [
        NudgeInvocation::Commit,
        NudgeInvocation::Base,
        NudgeInvocation::Main,
    ] {
        let (targeted, after, later, cycles) = run_scoped_then_file_change(NudgeRequestMsg {
            invocation,
            ..Default::default()
        });
        assert_scoped_then_file_change(targeted, after, later, cycles);
    }
}

#[test]
fn scenario_4_retry_bad_target() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    let target = "tests/pair.py";
    let fail = "tests/pair.py::test_bad";
    let timeout = "tests/pair.py::test_slow";
    let pass = "tests/pair.py::test_ok";
    let tests_dir = tmp.path().join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    std::fs::write(
        tests_dir.join("pair.py"),
        "def test_ok():\n    assert True\n\ndef test_bad():\n    assert True\n\ndef test_slow():\n    assert True\n",
    )
    .unwrap();
    assert!(
        crate::test_runner::workspace_selector_cache::store_python_workspace_selectors(
            tmp.path(),
            &[],
            &[pass.into(), fail.into(), timeout.into()],
            &[],
        )
    );
    seed_python_bad(
        tmp.path(),
        &[
            (fail, TestStatus::Failed),
            (timeout, TestStatus::TimedOut),
        ],
    );
    let tests = Arc::new(AtomicUsize::new(0));
    let (rx, sender) = nudge_after_cycles(
        Arc::clone(&tests),
        1,
        vec![NudgeRequestMsg {
            force_bad: true,
            targets: vec![target.into()],
            ..Default::default()
        }],
    );
    let tests_run = Arc::clone(&tests);
    let root = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let code = run_watch_loop_with(
        watch_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = tests_run.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                kiss::rust_llvm_cov_runner::emit_progress(&format!("PASS: {pass} (0.01s)"));
                kiss::rust_llvm_cov_runner::emit_progress(&format!("FAIL: {fail} (0.01s)"));
                kiss::rust_llvm_cov_runner::emit_progress(&format!("TIMEOUT: {timeout} (0.01s)"));
                kiss::rust_llvm_cov_runner::emit_progress(
                    "✗ 1 passed · 1 failed · 1 timed out · 1s total · 0s max pass",
                );
                return RunTestOnceOutcome::Code(1);
            }
            assert!(cycle_args.force_bad, "retry-bad must set force_bad");
            assert_eq!(
                cycle_args.invocation,
                TestInvocation::Targets(vec![target.to_string()])
            );
            let mut planned = crate::test_runner::empty_planned(root.clone(), Vec::new());
            crate::test_runner::apply_force_bad(&cycle_args, &mut planned).unwrap();
            let mut rerun = planned.prior_failure_selectors.python.clone();
            rerun.sort();
            assert_eq!(
                rerun,
                vec![fail.to_string(), timeout.to_string()],
                "retry-bad TARGET must rerun FAIL and TIMEOUT only"
            );
            assert!(
                !planned.sel.python.iter().any(|s| s == pass),
                "passing sibling must not be forced"
            );
            kiss::rust_llvm_cov_runner::emit_progress(&format!("FAIL: {fail} (0.01s)"));
            kiss::rust_llvm_cov_runner::emit_progress(&format!("TIMEOUT: {timeout} (0.01s)"));
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 1 timed out · 0.02s total · 0s max pass",
            );
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let replies = sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(tests.load(Ordering::SeqCst), 2);
    let out = replies[0].output.clone().unwrap_or_default();
    assert!(
        out.contains(fail) && out.contains(timeout) && !out.contains(pass),
        "scoped retry-bad recap must omit the passing sibling; out={out:?}"
    );
}

#[test]
fn scenario_5_lang_rust_then_bare() {
    assert_lang_then_bare(
        "rust",
        &["src/lib.rs::t_slow"],
        &["tests/a.py::test_a", "tests/b.py::test_b"],
    );
}

#[test]
fn scenario_6_lang_python_then_bare() {
    assert_lang_then_bare(
        "python",
        &["tests/a.py::test_a", "tests/b.py::test_b"],
        &["src/lib.rs::t_slow"],
    );
}
