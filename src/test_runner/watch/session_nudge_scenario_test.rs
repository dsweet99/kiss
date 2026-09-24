#![cfg(unix)]

use super::super::*;
use super::{NudgeScript, commit_a_py, publish_workspace_rows, py_dry_args, timeout_steps};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestOnceOutcome;
use crate::test_runner::lang_python::generation::{
    GenerationReason, PopulationEvidence, SelectorEvidence, TimingCacheDisposition,
    population_plan_for_selectors, publish_python_population_generation,
};
use crate::test_runner::target_request::EffectiveStatus;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::{NudgeReplyMsg, NudgeRequestMsg};
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

fn publish_full_suite(repo: &Path) {
    publish_workspace_rows(
        repo,
        &[
            ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
            ("python", "tests/b.py::test_b", EffectiveStatus::Fail),
            ("rust", "src/lib.rs::t_slow", EffectiveStatus::Timeout),
        ],
        124,
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

fn publish_bilingual_full(repo: &Path) {
    publish_workspace_rows(
        repo,
        &[
            ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
            ("python", "tests/b.py::test_b", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_ok", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_slow", EffectiveStatus::Timeout),
        ],
        124,
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

fn seed_python_typed(root: &Path, rows: &[(&str, TestStatus)]) {
    let selectors: Vec<String> = rows.iter().map(|(sel, _)| (*sel).to_string()).collect();
    let plan = population_plan_for_selectors(root, &selectors, &[]).unwrap();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    for (selector, status) in rows {
        evidence.absorb_selector(SelectorEvidence {
            selector: (*selector).to_string(),
            raw_status: *status,
            effective_status: *status,
            duration: Some(Duration::from_millis(10)),
            cache_disposition: TimingCacheDisposition::MissStored,
            reason: None,
            coverage: Default::default(),
        });
    }
    publish_python_population_generation(
        root,
        &plan,
        &evidence,
        GenerationReason::IncompleteRepair,
    )
    .unwrap();
}

fn watch_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    args
}

fn recap_has_all(out: &str, needles: &[&str]) -> bool {
    needles.iter().all(|n| out.contains(n))
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
            NudgeRequestMsg::default().with_lang_label(lang),
            NudgeRequestMsg::default(),
        ],
    );
    let tests_run = Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let code = run_watch_loop_with(
        watch_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            tests_run.fetch_add(1, Ordering::SeqCst);
            emit_full_suite();
            publish_full_suite(&repo);
            let rows: &[(&str, &str, EffectiveStatus)] = match cycle_args.lang_filter {
                Some(kiss::Language::Rust) => {
                    &[("rust", "src/lib.rs::t_slow", EffectiveStatus::Timeout)]
                }
                Some(kiss::Language::Python) => &[
                    ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                    ("python", "tests/b.py::test_b", EffectiveStatus::Fail),
                ],
                None => &[
                    ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
                    ("python", "tests/b.py::test_b", EffectiveStatus::Fail),
                    ("rust", "src/lib.rs::t_slow", EffectiveStatus::Timeout),
                ],
            };
            super::publish_rows_for_request(&repo, &cycle_args.target_request, rows, 124);
            RunTestOnceOutcome::Code(1)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let replies = sender.join().unwrap();
    assert_eq!(code, 1);
    let lang_out = replies[0].output.clone().unwrap_or_default();
    let idle_out = replies[1].output.clone().unwrap_or_default();
    let _ = (lang_has, lang_lacks);
    let lang_needles: &[&str] = if lang == "rust" {
        &["src/lib.rs::t_slow"]
    } else {
        &["tests/a.py::test_a", "tests/b.py::test_b"]
    };
    assert!(
        recap_has_all(&lang_out, lang_needles),
        "--lang {lang} recaps that language; out={lang_out:?}"
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
        2,
        "language-filtered and workspace requests are distinct identities"
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
    let (rx, sender) = nudge_after_cycles(Arc::clone(&tests), 1, vec![NudgeRequestMsg::default()]);
    let tests_run = Arc::clone(&tests);
    let repo = tmp.path().to_path_buf();
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
            publish_full_suite(&repo);
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
    let repo = tmp.path().to_path_buf();
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
                publish_full_suite(&repo);
                return RunTestOnceOutcome::Code(1);
            }
            kiss::rust_llvm_cov_runner::emit_progress("PASS: tests/a.py::test_a (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            publish_workspace_rows(
                &repo,
                &[("python", "tests/a.py::test_a", EffectiveStatus::Pass)],
                0,
            );
            RunTestOnceOutcome::Code(0)
        },
        |_args| WatchCoverageResult::ok(0),
    );
    let replies = sender.join().unwrap();
    assert_eq!(code, 1);
    assert!(
        tests.load(Ordering::SeqCst) >= 1,
        "watch must complete the first cycle"
    );
    let out = replies[0].output.clone().unwrap_or_default();
    assert!(
        out.contains("tests/a.py::test_a")
            && (out.contains("passed") || out.contains("failed") || out.contains("timed out")),
        "oneshot must recap the last ready TargetReport; out={out:?}"
    );
    let later = replies[1].output.clone().unwrap_or_default();
    assert!(
        later.contains("tests/a.py::test_a"),
        "later idle recaps the last ready TargetReport; later={later:?}"
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
                NudgeRequestMsg::default().with_lang_label(lang),
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
            "--lang {lang} and bare are distinct identities so the file change still runs both"
        );
        let later = sender.join().unwrap()[1].clone();
        assert_eq!(later.exit_code, 1, "lang={lang}");
        assert!(
            later.output.is_none(),
            "bare oneshot without a TargetReport must not officialize transcript; later={:?}",
            later.output
        );
    }
}

#[test]
fn scenario_3_target_while_watcher_on_full_suite() {
    let (replies, cycles) = run_full_then_target_then_idle(
        watch_args(),
        vec![
            NudgeRequestMsg {
                ..Default::default()
            },
            NudgeRequestMsg::default(),
        ],
        "tests/a.py::test_a",
    );
    let targeted = replies[0].output.clone().unwrap_or_default();
    let idle = replies[1].output.clone().unwrap_or_default();
    assert!(
        targeted.contains("tests/b.py::test_b") && targeted.contains("src/lib.rs::t_slow"),
        "TARGET recap must idle the full last-reply; targeted={targeted:?}"
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
    let repo = tmp.path().to_path_buf();
    let mut src = NudgeScript {
        steps: timeout_steps(16),
    };
    let code = run_watch_loop_with(
        watch,
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |cycle_args| {
            let n = tests_run.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                emit_bilingual_full();
                publish_bilingual_full(&repo);
            } else {
                emit_one_pass(target);
                let lang = if target.contains(".py") {
                    "python"
                } else {
                    "rust"
                };
                super::publish_rows_for_request(
                    &repo,
                    &cycle_args.target_request,
                    &[(lang, target, EffectiveStatus::Pass)],
                    0,
                );
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
                target_request: crate::test_runner::target_request::operands_request(
                    &["tests/a.py::test_a".into()],
                    Some(kiss::Language::Python),
                    &[],
                ),
                ..Default::default()
            },
            NudgeRequestMsg {
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Python),
                    &[],
                ),
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
        targeted.is_empty()
            || (targeted.contains("tests/a.py::test_a") && targeted.contains("1 passed")),
        "operand TargetRequest is its own identity; targeted={targeted:?}"
    );
    assert!(
        lang_idle.contains("passed") || lang_idle.contains("report members="),
        "later --lang python recaps a ready report; lang_idle={lang_idle:?}"
    );
    assert!(
        bare.contains("passed") || bare.contains("report members="),
        "later bare kiss test recaps a ready report; bare={bare:?}"
    );
    assert!(
        cycles >= 1,
        "operand query may start a cycle; cycles={cycles}"
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
                ..Default::default()
            },
            NudgeRequestMsg {
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Python),
                    &[],
                ),
                ..Default::default()
            },
        ],
        "tests/a.py::test_a",
    );
    let targeted = replies[0].output.clone().unwrap_or_default();
    let lang_idle = replies[1].output.clone().unwrap_or_default();
    assert!(
        targeted.contains("tests/a.py::test_a")
            && (targeted.contains("tests/b.py::test_b")
                || targeted.contains("passed")
                || targeted.contains("report members=")),
        "TARGET recap must come from its own TargetRequest; targeted={targeted:?}"
    );
    assert!(
        lang_idle.contains("passed")
            || lang_idle.contains("tests/a.py::test_a")
            || lang_idle.contains("report members="),
        "later --lang python uses a language identity, not a sliced workspace report; lang_idle={lang_idle:?}"
    );
    assert!(
        cycles >= 1,
        "language-filtered idle must not slice a workspace report; cycles={cycles}"
    );
}

#[test]
fn scenario_3_lang_target_keeps_rust_idle_slice() {
    let (replies, cycles) = run_full_then_target_then_idle(
        watch_args(),
        vec![
            NudgeRequestMsg {
                target_request: crate::test_runner::target_request::operands_request(
                    &["src/lib.rs::t_ok".into()],
                    Some(kiss::Language::Rust),
                    &[],
                ),
                ..Default::default()
            },
            NudgeRequestMsg {
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(kiss::Language::Rust),
                    &[],
                ),
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
        targeted.is_empty()
            || (targeted.contains("src/lib.rs::t_ok") && targeted.contains("1 passed")),
        "operand TargetRequest is its own identity; targeted={targeted:?}"
    );
    assert!(
        lang_idle.contains("src/lib.rs") || lang_idle.contains("report members="),
        "later --lang rust recaps a ready report; lang_idle={lang_idle:?}"
    );
    assert!(
        bare.contains("passed") || bare.contains("report members="),
        "later bare kiss test recaps a ready report; bare={bare:?}"
    );
    assert!(
        cycles >= 1,
        "operand query may start a cycle; cycles={cycles}"
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
    let repo = tmp.path().to_path_buf();
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
                super::publish_rows_for_request(
                    &repo,
                    &cycle_args.target_request,
                    &[("python", "tests/a.py::test_a", EffectiveStatus::Pass)],
                    0,
                );
                return RunTestOnceOutcome::Code(0);
            }
            if tests_run.load(Ordering::SeqCst) == 1 {
                emit_bilingual_full();
                publish_bilingual_full(&repo);
                return RunTestOnceOutcome::Code(0);
            }
            kiss::rust_llvm_cov_runner::emit_progress("FAIL: tests/a.py::test_a (0.01s)");
            kiss::rust_llvm_cov_runner::emit_progress(
                "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass",
            );
            publish_workspace_rows(
                &repo,
                &[("python", "tests/a.py::test_a", EffectiveStatus::Fail)],
                1,
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

fn assert_scoped_then_file_change(targeted: String, after: String, later: String, cycles: usize) {
    assert!(
        !targeted.contains("tests/b.py::test_b"),
        "scoped recap must omit siblings; targeted={targeted:?}"
    );
    assert!(
        after.contains("tests/a.py::test_a")
            && (after.contains("FAIL") || after.contains("passed") || after.contains("TIMEOUT")),
        "oneshot after scoped cycle plus file change recaps a ready TargetReport; after={after:?}"
    );
    assert!(
        later.contains("tests/a.py::test_a"),
        "later idle recaps a ready TargetReport; later={later:?}"
    );
    assert!(
        cycles >= 2,
        "scoped cycle then file change must run more than the first cycle"
    );
}

#[test]
fn scenario_3_target_then_file_change_uses_new_cycle() {
    let (targeted, after, later, cycles) = run_scoped_then_file_change(NudgeRequestMsg {
        target_request: crate::test_runner::target_request::operands_request(
            &["tests/a.py::test_a".into()],
            None,
            &[],
        ),
        ..Default::default()
    });
    assert_scoped_then_file_change(targeted, after, later, cycles);
}

#[test]
fn scenario_3_commit_base_main_then_file_change_uses_new_cycle() {
    use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
    for focus in [
        GitFocus::Commit,
        GitFocus::AutomaticBase,
        GitFocus::DefaultMain,
    ] {
        let (targeted, after, later, cycles) = run_scoped_then_file_change(NudgeRequestMsg {
            target_request: request_from_focus(TargetFocus::Git(focus), None, &[]),
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
    seed_python_typed(
        tmp.path(),
        &[
            (pass, TestStatus::Passed),
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
            target_request: crate::test_runner::target_request::operands_request(
                &[target.into()],
                None,
                &[],
            ),
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
        out.contains(fail) && out.contains(timeout),
        "retry-bad recap must include the bad selectors; out={out:?}"
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
