use super::*;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::test_mode_fixtures::python_dry_run_args;
use crate::test_runner::workspace_selector_cache::store_python_workspace_selectors;
use kiss::rust_llvm_cov_runner::WatchSuiteReport;
use kiss::rust_llvm_cov_runner::{ProgressLanguageGuard, emit_progress};
use std::path::Path;

fn collect_and_cache(root: &Path) -> Vec<String> {
    let selectors =
        kiss::rpytest_runner::collect_pytest_nodeids(kiss::rpytest_runner::PytestCollectRequest {
            cwd: root.to_path_buf(),
            python: "python".into(),
            paths: Vec::new(),
            pytest_args: Vec::new(),
            env: Default::default(),
        })
        .unwrap()
        .nodeids;
    assert!(store_python_workspace_selectors(root, &[], &selectors, &[]));
    selectors
}

fn run_inventory_cycle(root: &Path, suite: &mut WatchSuiteReport, last: &mut LastReplies) {
    run_inventory_cycle_for_language(root, suite, last, None);
}

fn run_inventory_cycle_for_language(
    root: &Path,
    suite: &mut WatchSuiteReport,
    last: &mut LastReplies,
    lang: Option<kiss::Language>,
) {
    let selectors = collect_and_cache(root);
    run_selector_cycle(root, suite, last, lang, &selectors);
}

fn run_selector_cycle(
    root: &Path,
    suite: &mut WatchSuiteReport,
    last: &mut LastReplies,
    lang: Option<kiss::Language>,
    selectors: &[String],
) {
    run_progress_cycle(root, suite, last, lang, || {
        let _lang = ProgressLanguageGuard::enter(lang.unwrap_or(kiss::Language::Python));
        for selector in selectors {
            emit_progress(&format!("PASS: {selector} (0.01s)"));
        }
        crate::test_runner::final_summary::print_final_test_summary(
            &crate::test_runner::final_summary::FinalTestSummary {
                passed: selectors.len(),
                ..Default::default()
            },
            Duration::ZERO,
        );
    });
}

fn run_progress_cycle(
    root: &Path,
    suite: &mut WatchSuiteReport,
    last: &mut LastReplies,
    lang: Option<kiss::Language>,
    mut progress: impl FnMut(),
) {
    let mut args = python_dry_run_args(Vec::new());
    args.dry_run = false;
    args.invocation = TestInvocation::All;
    args.lang_filter = lang;
    let mut live = live_from_args_disabled(args, Duration::ZERO, root);
    let mut source = super::super::event_source::FakeWatchEventSource {
        events: vec![],
        disconnected: None,
    };
    let mut filter = WatchPathFilter::build(root, &[], None, &TestInvocation::All);
    let outcome = run_one_watch_cycle(WatchCycleCtx {
        live: &mut live,
        queued: &mut None,
        source: &mut source,
        filter: &mut filter,
        machine: &mut SettleMachine::new(Duration::ZERO),
        repo_root: root,
        last_reply: last,
        suite,
        run_cycle: &mut |_: RunTestCmdArgs<'_>| {
            progress();
            RunTestOnceOutcome::Code(0)
        },
        run_cov: &mut |_: &RunTestCmdArgs<'_>, _: &WatchLiveConfig| WatchCoverageResult::ok(0),
        reuse_suite: false,
    });
    assert!(matches!(outcome, CycleOutcome::Continue));
}

fn check_edit(edit: impl FnOnce(&Path)) {
    let _serial = watch_loop_serial();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("test_keep.py"), "def test_keep():\n    pass\n").unwrap();
    std::fs::write(root.join("test_edit.py"), "def test_old():\n    pass\n").unwrap();
    let mut suite = WatchSuiteReport::default();
    let mut last = LastReplies::for_repo(root);
    run_inventory_cycle(root, &mut suite, &mut last);
    assert!(
        last.get(None)
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .contains("test_old")
    );
    edit(root);
    run_inventory_cycle(root, &mut suite, &mut last);
    let current = collect_and_cache(root);
    for lang in [None, Some(kiss::Language::Python)] {
        let reply = last.get(lang).unwrap();
        let output = reply.output.as_ref().unwrap();
        assert!(
            !output.contains("test_old"),
            "obsolete test in {lang:?} reply: {output}"
        );
        assert!(
            output.contains(&format!("{} passed", current.len())),
            "{output}"
        );
        for selector in &current {
            assert!(output.contains(selector), "missing {selector}: {output}");
        }
        assert_eq!(reply.exit_code, 0);
    }
}

#[test]
fn watcher_forgets_deleted_test_function() {
    check_edit(|root| std::fs::write(root.join("test_edit.py"), "").unwrap());
}

#[test]
fn watcher_forgets_deleted_test_file() {
    check_edit(|root| std::fs::remove_file(root.join("test_edit.py")).unwrap());
}

#[test]
fn watcher_forgets_final_test_files() {
    check_edit(|root| {
        std::fs::remove_file(root.join("test_edit.py")).unwrap();
        std::fs::remove_file(root.join("test_keep.py")).unwrap();
    });
}

#[test]
fn collection_refreshes_same_length_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test_edit.py");
    std::fs::write(&path, "def test_old():\n    pass\n").unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(collect_and_cache(tmp.path()), ["test_edit.py::test_old"]);
    std::fs::write(&path, "def test_new():\n    pass\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(collect_and_cache(tmp.path()), ["test_edit.py::test_new"]);
}

#[test]
fn stale_or_different_inventory_keeps_prior_results() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test_keep.py");
    std::fs::write(&path, "def test_keep():\n    pass\n").unwrap();
    collect_and_cache(tmp.path());
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&["FAIL: test_old.py::test_old".into()]);
    let mut last = LastReplies::for_repo(tmp.path());
    let args = python_dry_run_args(Vec::new());
    last.stamp_session(&["other".into()], &[], &[]);
    assert!(super::super::session_cycle::reconcile_inventory(&mut suite, &last, &args));
    assert_eq!(suite.failed(), 1);
    last.stamp_session(&[], &[], &[]);
    std::fs::write(&path, "def test_changed():\n    pass\n").unwrap();
    assert!(super::super::session_cycle::reconcile_inventory(&mut suite, &last, &args));
    assert_eq!(suite.failed(), 1);
}

#[test]
fn inventory_prunes_all_outcomes_but_keeps_current_and_rust_tests() {
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS: test_old.py::test_pass".into(),
        "FAIL: test_old.py::test_fail".into(),
        "TIMEOUT: test_old.py::test_timeout".into(),
        "FAIL: test_keep.py::test_keep".into(),
        "TIMEOUT: src/lib.rs::test_rust".into(),
    ]);
    suite.retain_language_selectors(kiss::Language::Python, &["test_keep.py::test_keep".into()]);
    let output = suite.format();
    assert!(!output.contains("test_old.py"), "{output}");
    assert!(output.contains("test_keep.py::test_keep"), "{output}");
    assert!(output.contains("src/lib.rs::test_rust"), "{output}");
    assert_eq!(
        (suite.passed(), suite.failed(), suite.timed_out()),
        (0, 1, 1)
    );
}

#[test]
fn empty_python_inventory_clears_collapsed_counts_not_rust() {
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS (cached): 70 selectors".into(),
        "kiss test: lang_collapsed python pass 70".into(),
        "FAIL (cached): 2 selectors".into(),
        "kiss test: lang_collapsed python fail 2".into(),
        "TIMEOUT (cached): 1 selectors".into(),
        "kiss test: lang_collapsed python timeout 1".into(),
        "PASS: src/lib.rs::test_rust".into(),
        "✓ 71 passed · 2 failed · 1 timed out · 0s total · 0s max pass".into(),
    ]);
    suite.retain_language_selectors(kiss::Language::Python, &[]);
    assert_eq!(
        (suite.passed(), suite.failed(), suite.timed_out()),
        (1, 0, 0)
    );
    let (code, python) = suite.try_format_language(kiss::Language::Python).unwrap();
    assert_eq!(code, 0);
    assert!(python.contains("0 passed"), "{python}");
    assert!(suite.format().contains("src/lib.rs::test_rust"));
}

#[test]
fn partial_named_inventory_keeps_collapsed_results() {
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS (cached): 2 selectors".into(),
        "kiss test: lang_collapsed python pass 2".into(),
    ]);
    suite.retain_language_selectors(kiss::Language::Python, &[
        "test_keep.py::test_a".into(),
        "test_keep.py::test_b".into(),
    ]);
    let (_, output) = suite.try_format_language(kiss::Language::Python).unwrap();
    assert!(output.contains("2 passed"), "{output}");
    assert_eq!(suite.passed(), 2);
}

#[test]
fn watcher_deletion_replaces_collapsed_python_counts() {
    let _serial = watch_loop_serial();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("test_keep.py"), "def test_keep():\n    pass\n").unwrap();
    std::fs::write(root.join("test_edit.py"), "def test_old():\n    pass\n").unwrap();
    collect_and_cache(root);
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS (cached): 2 selectors".into(),
        "kiss test: lang_collapsed python pass 2".into(),
    ]);
    let mut last = LastReplies::for_repo(root);
    std::fs::remove_file(root.join("test_edit.py")).unwrap();
    run_inventory_cycle(root, &mut suite, &mut last);
    for lang in [None, Some(kiss::Language::Python)] {
        let reply = last.get(lang).unwrap();
        let output = reply.output.as_ref().unwrap();
        assert!(output.contains("1 passed"), "{lang:?}: {output}");
    }
}

#[test]
fn python_scoped_deletion_preserves_only_current_bilingual_counts() {
    let _serial = watch_loop_serial();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("test_keep.py"), "def test_keep():\n    pass\n").unwrap();
    std::fs::write(root.join("test_edit.py"), "def test_old():\n    pass\n").unwrap();
    collect_and_cache(root);
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS (cached): 2 selectors".into(),
        "kiss test: lang_collapsed python pass 2".into(),
        "PASS (cached): 3 selectors".into(),
        "kiss test: lang_collapsed rust pass 3".into(),
        "✓ 5 passed · 0 failed · 0 timed out · 0s total · 0s max pass".into(),
    ]);
    let mut last = LastReplies::for_repo(root);
    std::fs::remove_file(root.join("test_edit.py")).unwrap();
    run_inventory_cycle_for_language(root, &mut suite, &mut last, Some(kiss::Language::Python));
    for (lang, count) in [(None, 4), (Some(kiss::Language::Python), 1), (Some(kiss::Language::Rust), 3)] {
        let reply = last.get(lang).unwrap();
        let output = reply.output.as_ref().unwrap();
        assert!(output.contains(&format!("{count} passed")), "{lang:?}: {output}");
    }
}

fn check_rust_edit(source: Option<&str>, expected: &[&str]) {
    let _serial = watch_loop_serial();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let path = root.join("lib.rs");
    std::fs::write(&path, "#[test] fn test_old() {}\n#[test] fn test_keep() {}\n").unwrap();
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "FAIL: lib.rs::test_old".into(),
        "PASS: lib.rs::test_keep".into(),
        "PASS: test_python.py::test_keep".into(),
    ]);
    let mut last = LastReplies::for_repo(root);
    match source {
        Some(source) => std::fs::write(&path, source).unwrap(),
        None => std::fs::remove_file(&path).unwrap(),
    }
    let selectors = crate::test_runner::runners::enumerate_workspace_rust_selectors(root, &[]).unwrap();
    assert!(crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
        root, &[], &selectors,
    ));
    let reports = expected.iter().map(|name| format!("lib.rs::{name}")).collect::<Vec<_>>();
    run_selector_cycle(root, &mut suite, &mut last, Some(kiss::Language::Rust), &reports);
    for (lang, count) in [(None, expected.len() + 1), (Some(kiss::Language::Rust), expected.len())] {
        let reply = last.get(lang).unwrap();
        let output = reply.output.as_ref().unwrap();
        assert!(!output.contains("test_old"), "{output}");
        assert!(output.contains(&format!("{count} passed · 0 failed")), "{output}");
        assert_eq!(reply.exit_code, 0);
        for selector in &reports {
            assert!(output.contains(selector), "{output}");
        }
    }
    assert!(last.get(None).unwrap().output.as_ref().unwrap().contains("test_python.py::test_keep"));
}

#[test]
fn watcher_prunes_deleted_rust_test_from_bilingual_reply() {
    check_rust_edit(Some("#[test] fn test_keep() {}\n"), &["test_keep"]);
}

#[test]
fn watcher_prunes_renamed_rust_test_from_bilingual_reply() {
    check_rust_edit(Some("#[test] fn test_new() {}\n#[test] fn test_keep() {}\n"), &["test_new", "test_keep"]);
}

#[test]
fn watcher_prunes_final_rust_test_file_from_bilingual_reply() {
    check_rust_edit(None, &[]);
}

#[test]
fn rust_inventory_normalizes_logical_ids_and_clears_collapsed_counts() {
    let mut suite = WatchSuiteReport::default();
    suite.merge_lines(&[
        "PASS (cached): 2 selectors".into(),
        "kiss test: lang_collapsed rust pass 2".into(),
        "FAIL: tests::test_old".into(),
        "PASS: tests::test_keep".into(),
    ]);
    suite.retain_rust_selectors(
        &["tests::test_keep".into()],
        &[("tests::test_keep".into(), "lib.rs::test_keep".into())].into(),
    );
    let (_, rust) = suite.try_format_language(kiss::Language::Rust).unwrap();
    assert!(rust.contains("1 passed · 0 failed"), "{rust}");
    assert!(rust.contains("lib.rs::test_keep"), "{rust}");
    assert!(!rust.contains("test_old"), "{rust}");
}

fn check_deletion_report_transition(collapsed_after: bool) {
    let _serial = watch_loop_serial();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let path = root.join("test_edit.py");
    std::fs::write(&path, "def test_old(): pass\ndef test_keep(): pass\n").unwrap();
    collect_and_cache(root);
    let mut suite = WatchSuiteReport::default();
    let python_before: Vec<String> = if collapsed_after {
        vec!["PASS: test_edit.py::test_old".into(), "PASS: test_edit.py::test_keep".into()]
    } else {
        vec!["PASS (cached): 2 selectors".into(), "kiss test: lang_collapsed python pass 2".into()]
    };
    suite.merge_lines(&python_before);
    suite.merge_lines(&[
        "PASS (cached): 3 selectors".into(),
        "kiss test: lang_collapsed rust pass 3".into(),
        "✓ 5 passed · 0 failed · 0 timed out · 0s total · 0s max pass".into(),
    ]);
    std::fs::write(&path, "def test_keep(): pass\n").unwrap();
    collect_and_cache(root);
    let mut last = LastReplies::for_repo(root);
    run_progress_cycle(root, &mut suite, &mut last, None, || {
        for (lang, count) in [(kiss::Language::Python, 1), (kiss::Language::Rust, 3)] {
            let _lang = ProgressLanguageGuard::enter(lang);
            if lang == kiss::Language::Python && !collapsed_after {
                emit_progress("PASS (cached): test_edit.py::test_keep");
            } else {
                emit_progress(&format!("PASS (cached): {count} selectors"));
            }
        }
        crate::test_runner::final_summary::print_final_test_summary(
            &crate::test_runner::final_summary::FinalTestSummary { passed: 4, ..Default::default() },
            Duration::ZERO,
        );
    });
    assert_eq!(suite.passed(), 4, "{}", suite.format());
    for (lang, count) in [(None, 4), (Some(kiss::Language::Python), 1), (Some(kiss::Language::Rust), 3)] {
        let output = last.get(lang).unwrap().output.as_ref().unwrap();
        assert!(output.contains(&format!("{count} passed")), "{lang:?}: {output}");
        assert!(!output.contains("test_old"), "{output}");
    }
}

#[test]
fn deletion_with_collapsed_report_preserves_foreign_counts() {
    check_deletion_report_transition(true);
}

#[test]
fn deletion_with_named_report_preserves_foreign_counts() {
    check_deletion_report_transition(false);
}

#[test]
fn watcher_forgets_renamed_test() {
    check_edit(|root| {
        std::fs::write(root.join("test_edit.py"), "def test_new():\n    pass\n").unwrap();
    });
}
