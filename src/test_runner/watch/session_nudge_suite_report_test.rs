use super::{NudgeScript, commit_a_py, py_dry_args, timeout_steps};
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::test_mode_fixtures::init_git;
use crate::test_runner::watch::control::{NudgeRequest, NudgeRequestMsg};
use crate::test_runner::{RunTestOnceOutcome, WatchCoverageResult, run_kiss_test_report_reuse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn watch_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = py_dry_args();
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    args
}

fn rust_watch_args() -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = watch_args();
    args.set_lang_filter(Some(kiss::Language::Rust));
    args
}

fn emit_rslip_then_summary() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
    crate::test_runner::emit_test_progress("kiss test: tests_remaining=2");
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 3,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

#[test]
fn watch_to_waiting_then_oneshot_skips_rslip() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    std::fs::write(
        tmp.path().join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         test_coverage_threshold = 0\n\
         orphan_detection = false\n",
    )
    .unwrap();
    commit_a_py(&tmp);
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let mut src = NudgeScript {
        steps: timeout_steps(4),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        rust_watch_args(),
        Duration::from_millis(20),
        tmp.path(),
        &mut src,
        None,
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            emit_rslip_then_summary()
        },
        |_| WatchCoverageResult::ok(0),
    );
    assert_eq!(code, 1);
    assert_eq!(watch_runs.load(Ordering::SeqCst), 1);
    let request = crate::test_runner::target_request::request_from_run_args(&rust_watch_args());
    assert!(
        crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
            tmp.path(),
            &[],
            &[],
        )
    );
    let built = crate::test_runner::target_request::materialize_target_report(
        tmp.path(),
        &request,
        &crate::test_runner::target_request::EnsurePolicy {
            dry_run: false,
            require_complete: false,
            inject_mismatch: false,
            retry_bad: false,
            coverage_all: false,
            assemble_only: false,
        },
    )
    .unwrap();
    crate::test_runner::target_request::publish_if_rows_hold(tmp.path(), &request, &built).unwrap();
    let oneshot = run_kiss_test_report_reuse(
        rust_watch_args(),
        |_| panic!("watch-dead oneshot must not re-enter the engine"),
        |_| panic!("watch-dead oneshot must not run coverage"),
        true,
        Some(tmp.path()),
    );
    assert_eq!(oneshot.exit_code, 0);
    let replayed = oneshot.output.unwrap_or_default();
    assert!(
        replayed.contains("report members=") || replayed.contains("passed"),
        "{replayed}"
    );
    assert!(!replayed.contains("rslip prepared"), "{replayed}");
    assert!(!replayed.contains("tests_remaining"), "{replayed}");
}

fn emit_bilingual_run() -> RunTestOnceOutcome {
    crate::test_runner::emit_test_progress("kiss test: rslip prepared hits=2 misses=0");
    crate::test_runner::emit_test_progress("kiss test: tests_remaining=2");
    {
        let _guard =
            kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Python);
        crate::test_runner::emit_test_progress("PASS (cached): 2 selectors");
    }
    {
        let _guard = kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Rust);
        crate::test_runner::emit_test_progress("PASS (cached): 3 selectors");
    }
    crate::test_runner::final_summary::print_final_test_summary(
        &crate::test_runner::final_summary::FinalTestSummary {
            passed: 5,
            failed: 0,
            ..crate::test_runner::final_summary::FinalTestSummary::default()
        },
        Duration::from_millis(10),
    );
    RunTestOnceOutcome::Code(0)
}

fn persist_with(repo: &std::path::Path, args: crate::test_runner::RunTestCmdArgs<'static>) {
    publish_ready_for_args(repo, &args);
}

fn publish_ready_for_args(
    repo: &std::path::Path,
    args: &crate::test_runner::RunTestCmdArgs<'static>,
) {
    use super::publish_rows_for_request;
    use crate::test_runner::target_request::{EffectiveStatus, request_from_run_args};
    let request = request_from_run_args(args);
    publish_rows_for_request(
        repo,
        &request,
        &[
            ("python", "tests/a.py::test_a", EffectiveStatus::Pass),
            ("python", "tests/b.py::test_b", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_ok", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_slow", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_other", EffectiveStatus::Pass),
        ],
        0,
    );
}

fn publish_timeout_ready(repo: &std::path::Path) {
    use super::publish_workspace_rows;
    use crate::test_runner::target_request::EffectiveStatus;
    publish_workspace_rows(
        repo,
        &[
            ("python", "tests/a.py::t", EffectiveStatus::Timeout),
            ("rust", "src/lib.rs::t_ok", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_a", EffectiveStatus::Pass),
            ("rust", "src/lib.rs::t_b", EffectiveStatus::Pass),
        ],
        124,
    );
}

fn scoped_args(lang: kiss::Language) -> crate::test_runner::RunTestCmdArgs<'static> {
    let mut args = watch_args();
    args.set_lang_filter(Some(lang));
    args
}

fn send_nudge(
    msg: NudgeRequestMsg,
) -> (
    mpsc::Receiver<NudgeRequest>,
    std::thread::JoinHandle<crate::test_runner::watch::control::NudgeReplyMsg>,
) {
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        tx.send(NudgeRequest {
            msg,
            reply: reply_tx,
        })
        .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(5)).unwrap()
    });
    (rx, sender)
}

fn idle_watch(
    repo: &std::path::Path,
    args: crate::test_runner::RunTestCmdArgs<'static>,
    rx: &mpsc::Receiver<NudgeRequest>,
) -> usize {
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        args,
        Duration::from_secs(3600),
        repo,
        &mut src,
        Some(rx),
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            panic!("watcher must reuse the oneshot DurableSuiteRecap; engine must not run");
        },
        |_| panic!("watcher must not run coverage after a fresh oneshot recap"),
    );
    assert_eq!(code, 1);
    watch_runs.load(Ordering::SeqCst)
}

fn assert_lang_recap(out: &str, _passed: &str, _other: &str) {
    assert!(
        out.contains("passed") || out.contains("report members="),
        "idle must recap TargetReport; {out}"
    );
    assert!(!out.contains("rslip prepared"), "{out}");
}

fn lang_msg(lang: Option<&str>) -> NudgeRequestMsg {
    let filter = match lang {
        Some("python") => Some(kiss::Language::Python),
        Some("rust") => Some(kiss::Language::Rust),
        _ => None,
    };
    NudgeRequestMsg {
        target_request: crate::test_runner::target_request::workspace_request(filter, &[]),
        ..Default::default()
    }
}

fn seed_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    commit_a_py(&tmp);
    tmp
}

#[test]
fn oneshot_then_watch_then_lang_python_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    publish_ready_for_args(tmp.path(), &scoped_args(kiss::Language::Python));
    let (rx, sender) = send_nudge(lang_msg(Some("python")));
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    assert_lang_recap(
        &sender.join().unwrap().output.unwrap_or_default(),
        "2 passed",
        "3 passed",
    );
}

#[test]
fn oneshot_then_watch_then_lang_rust_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    publish_ready_for_args(tmp.path(), &scoped_args(kiss::Language::Rust));
    let (rx, sender) = send_nudge(lang_msg(Some("rust")));
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    assert_lang_recap(
        &sender.join().unwrap().output.unwrap_or_default(),
        "3 passed",
        "2 passed",
    );
}

#[test]
fn oneshot_then_watch_then_bare_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    let (rx, sender) = send_nudge(lang_msg(None));
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    let out = sender.join().unwrap().output.unwrap_or_default();
    assert!(
        out.contains("passed") || out.contains("report members="),
        "{out}"
    );
    assert!(!out.contains("rslip prepared"), "{out}");
}

#[test]
fn oneshot_then_watch_lang_then_lang_then_bare_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    publish_ready_for_args(tmp.path(), &scoped_args(kiss::Language::Python));
    publish_ready_for_args(tmp.path(), &scoped_args(kiss::Language::Rust));
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        let mut replies = Vec::new();
        for lang in [Some("python"), Some("rust"), None] {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(NudgeRequest {
                msg: lang_msg(lang),
                reply: reply_tx,
            })
            .unwrap();
            replies.push(reply_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        replies
    });
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    let replies = sender.join().unwrap();
    assert_lang_recap(
        &replies[0].output.clone().unwrap_or_default(),
        "2 passed",
        "3 passed",
    );
    assert_lang_recap(
        &replies[1].output.clone().unwrap_or_default(),
        "3 passed",
        "2 passed",
    );
    assert!(
        replies[2]
            .output
            .clone()
            .unwrap_or_default()
            .contains("5 passed")
    );
}

#[test]
fn python_only_oneshot_leaves_unscoped_watch_to_run() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), scoped_args(kiss::Language::Python));
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let mut src = NudgeScript {
        steps: timeout_steps(4),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        watch_args(),
        Duration::from_millis(20),
        tmp.path(),
        &mut src,
        None,
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        },
        |_| WatchCoverageResult::ok(0),
    );
    assert_eq!(code, 1);
    assert_eq!(watch_runs.load(Ordering::SeqCst), 1);
}

#[test]
fn oneshot_then_watch_retry_bad_runs_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    let (rx, sender) = send_nudge(NudgeRequestMsg {
        force_bad: true,
        ..Default::default()
    });
    let watch_runs = Arc::new(AtomicUsize::new(0));
    let watch_runs_cycle = Arc::clone(&watch_runs);
    let mut src = NudgeScript {
        steps: timeout_steps(12),
    };
    let code = crate::test_runner::watch::run_watch_loop_with(
        watch_args(),
        Duration::from_secs(3600),
        tmp.path(),
        &mut src,
        Some(&rx),
        move |_| {
            watch_runs_cycle.fetch_add(1, Ordering::SeqCst);
            emit_bilingual_run()
        },
        |_| WatchCoverageResult::ok(0),
    );
    let _ = sender.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(watch_runs.load(Ordering::SeqCst), 1);
}

#[test]
fn oneshot_then_watch_metrics_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    let (rx, sender) = send_nudge(NudgeRequestMsg {
        metrics: true,
        ..Default::default()
    });
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    let _ = sender.join().unwrap();
}

#[test]
fn oneshot_then_watch_python_twice_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), watch_args());
    publish_ready_for_args(tmp.path(), &scoped_args(kiss::Language::Python));
    let (tx, rx) = mpsc::channel::<NudgeRequest>();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        let mut replies = Vec::new();
        for _ in 0..2 {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(NudgeRequest {
                msg: lang_msg(Some("python")),
                reply: reply_tx,
            })
            .unwrap();
            replies.push(reply_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        replies
    });
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    let replies = sender.join().unwrap();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0].output, replies[1].output);
    assert_lang_recap(
        &replies[0].output.clone().unwrap_or_default(),
        "2 passed",
        "3 passed",
    );
}

#[test]
fn oneshot_timeout_then_watch_bare_keeps_exit_124() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    let runs = AtomicUsize::new(0);
    let first = run_kiss_test_report_reuse(
        watch_args(),
        |_a| {
            runs.fetch_add(1, Ordering::SeqCst);
            {
                let _g = kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(
                    kiss::Language::Python,
                );
                crate::test_runner::emit_test_progress("TIMEOUT: tests/a.py::t (1s)");
            }
            {
                let _g =
                    kiss::rust_llvm_cov_runner::ProgressLanguageGuard::enter(kiss::Language::Rust);
                crate::test_runner::emit_test_progress("PASS (cached): 3 selectors");
            }
            crate::test_runner::final_summary::print_final_test_summary(
                &crate::test_runner::final_summary::FinalTestSummary {
                    passed: 3,
                    timed_out_selectors: vec!["tests/a.py::t".into()],
                    ..crate::test_runner::final_summary::FinalTestSummary::default()
                },
                Duration::from_millis(10),
            );
            RunTestOnceOutcome::Code(124)
        },
        |_a| panic!("must not run_cov"),
        true,
        Some(tmp.path()),
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(first.exit_code, 124);
    publish_timeout_ready(tmp.path());
    let (rx, sender) = send_nudge(lang_msg(None));
    assert_eq!(idle_watch(tmp.path(), watch_args(), &rx), 0);
    assert_eq!(sender.join().unwrap().exit_code, 124);
}

#[test]
fn rust_only_oneshot_then_watch_lang_rust_skips_engine() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = seed_repo();
    persist_with(tmp.path(), scoped_args(kiss::Language::Rust));
    let (rx, sender) = send_nudge(NudgeRequestMsg {
        target_request: crate::test_runner::target_request::workspace_request(
            Some(kiss::Language::Rust),
            &[],
        ),
        ..Default::default()
    });
    assert_eq!(
        idle_watch(tmp.path(), scoped_args(kiss::Language::Rust), &rx),
        0
    );
    assert_lang_recap(
        &sender.join().unwrap().output.unwrap_or_default(),
        "3 passed",
        "2 passed",
    );
}
