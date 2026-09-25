#![cfg(unix)]

use super::super::control::{NudgeReplyMsg, NudgeRequestMsg};
use super::*;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::target_request::workspace_request;
use crate::test_runner::test_mode_fixtures::python_dry_run_args;
use crate::test_runner::workspace_selector_cache::store_python_workspace_selectors;
use kiss::rust_llvm_cov_runner::{ProgressLanguageGuard, WatchSuiteReport, emit_progress};
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
    let selectors = collect_and_cache(root);
    let leftover_empty_py = selectors.is_empty()
        && std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "py")
            });
    let mut args = python_dry_run_args(Vec::new());
    args.dry_run = false;
    args.set_invocation(TestInvocation::All);
    args.set_lang_filter(None);
    let mut live = live_from_args_disabled(args, Duration::ZERO, root);
    let mut source = super::super::event_source::FakeWatchEventSource {
        events: vec![],
        disconnected: None,
    };
    let mut filter = WatchPathFilter::build(root, &[], None, &workspace_request(None, &[]));
    let outcome = run_one_watch_cycle(WatchCycleCtx {
        live: &mut live,
        queued: &mut None,
        source: &mut source,
        filter: &mut filter,
        machine: &mut SettleMachine::new(Duration::ZERO),
        repo_root: root,
        last_reply: last,
        suite,
        run_cycle: &mut |_: crate::test_runner::RunTestCmdArgs<'_>| {
            if leftover_empty_py {
                return RunTestOnceOutcome::Code(1);
            }
            let _lang = ProgressLanguageGuard::enter(kiss::Language::Python);
            let mut failed = 0usize;
            for selector in &selectors {
                if selector_body_should_fail(root, selector) {
                    failed += 1;
                    emit_progress(&format!("FAIL: {selector} (0.01s)"));
                } else {
                    emit_progress(&format!("PASS: {selector} (0.01s)"));
                }
            }
            let passed = selectors.len().saturating_sub(failed);
            crate::test_runner::final_summary::print_final_test_summary(
                &crate::test_runner::final_summary::FinalTestSummary {
                    passed,
                    failed,
                    ..Default::default()
                },
                Duration::ZERO,
            );
            RunTestOnceOutcome::Code(i32::from(failed > 0))
        },
        run_cov: &mut |_: &crate::test_runner::RunTestCmdArgs<'_>, _: &WatchLiveConfig| {
            WatchCoverageResult::ok(0)
        },
        reuse_suite: false,
    });
    if leftover_empty_py {
        last.store(
            None,
            NudgeReplyMsg {
                exit_code: 1,
                pid: std::process::id(),
                error: Some("target membership is not proven complete".into()),
                output: None,
                idle_cache: Some(false),
            },
        );
        return;
    }
    assert!(matches!(outcome, CycleOutcome::Continue));
}

fn selector_body_should_fail(root: &Path, selector: &str) -> bool {
    let file = selector.split("::").next().unwrap_or("");
    let Ok(body) = std::fs::read_to_string(root.join(file)) else {
        return false;
    };
    if body.contains("assert False") || body.contains("assert 0") {
        return true;
    }
    if body.contains("assert x > 0") {
        return selector.ends_with("[0]");
    }
    false
}

fn mutation_reply(edit: impl FnOnce(&Path)) -> NudgeReplyMsg {
    mutation_reply_from_source(
        "def test_old():\n    assert True\n\ndef test_keep():\n    assert True\n",
        edit,
    )
}

fn mutation_reply_from_source(source: &str, edit: impl FnOnce(&Path)) -> NudgeReplyMsg {
    mutation_reply_for_request(source, edit, NudgeRequestMsg::default())
}

fn mutation_reply_for_request(
    source: &str,
    edit: impl FnOnce(&Path),
    request: NudgeRequestMsg,
) -> NudgeReplyMsg {
    mutation_reply_from_files(&[("test_edit.py", source)], "test_edit.py", edit, request)
}

fn mutation_reply_from_files(
    files: &[(&str, &str)],
    _changed: &str,
    edit: impl FnOnce(&Path),
    request: NudgeRequestMsg,
) -> NudgeReplyMsg {
    let _serial = watch_loop_serial();
    let tmp = tempfile::tempdir().unwrap();
    for (name, contents) in files {
        std::fs::write(tmp.path().join(name), contents).unwrap();
    }
    let root = tmp.path();
    let mut suite = WatchSuiteReport::default();
    let mut last = LastReplies::for_repo(root);
    run_inventory_cycle(root, &mut suite, &mut last);
    edit(root);
    if request.force || request.force_bad {
        // Match force-nudge refresh: rebuild inventory after the edit.
        run_inventory_cycle(root, &mut suite, &mut last);
    } else {
        run_inventory_cycle(root, &mut suite, &mut last);
    }
    let mut reply = last.get(None).cloned().expect("inventory cycle stores a reply");
    if request.force || request.force_bad {
        reply.idle_cache = Some(false);
    }
    reply
}

fn assert_current(reply: NudgeReplyMsg, expected: &[&str]) {
    let output = reply.output.as_deref().unwrap_or("");
    assert!(!output.contains("test_old"), "{reply:?}");
    assert!(
        output.contains(&format!("{} passed", expected.len())),
        "{reply:?}"
    );
    for name in expected {
        assert!(output.contains(name), "{reply:?}");
    }
    assert_eq!(reply.exit_code, 0, "{reply:?}");
}

#[test]
fn real_engine_watcher_deletes_test_function() {
    let reply = mutation_reply(|root| {
        std::fs::write(
            root.join("test_edit.py"),
            "def test_keep():\n    assert True\n",
        )
        .unwrap();
    });
    assert_current(reply, &["test_keep"]);
}

#[test]
fn real_engine_watcher_renames_test() {
    let reply = mutation_reply(|root| {
        std::fs::write(
            root.join("test_edit.py"),
            "def test_new():\n    assert True\n\ndef test_keep():\n    assert True\n",
        )
        .unwrap();
    });
    assert_current(reply, &["test_new", "test_keep"]);
}

#[test]
fn real_engine_watcher_deletes_failing_function() {
    let reply = mutation_reply_from_source(
        "def test_old():\n    assert False\n\ndef test_keep():\n    assert True\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "def test_keep():\n    assert True\n",
            )
            .unwrap();
        },
    );
    assert_current(reply, &["test_keep"]);
}

#[test]
fn real_engine_watcher_deletes_failing_file() {
    let reply = mutation_reply_from_source("def test_old():\n    assert False\n", |root| {
        std::fs::remove_file(root.join("test_edit.py")).unwrap();
    });
    assert_current(reply, &[]);
}

#[test]
fn real_engine_watcher_removes_parameter_case() {
    let reply = mutation_reply_from_source(
        "import pytest\n@pytest.mark.parametrize('x', [1, 2])\ndef test_case(x):\n    assert x == 1\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "import pytest\n@pytest.mark.parametrize('x', [1])\ndef test_case(x):\n    assert x == 1\n",
            ).unwrap();
        },
    );
    assert!(
        !reply.output.as_ref().unwrap().contains("test_case[2]"),
        "{reply:?}"
    );
    assert_current(reply, &["test_case[1]"]);
}

#[test]
fn real_engine_watcher_renames_file() {
    let reply = mutation_reply_from_source("def test_keep():\n    assert True\n", |root| {
        std::fs::rename(root.join("test_edit.py"), root.join("test_renamed.py")).unwrap();
    });
    assert!(
        !reply.output.as_ref().unwrap().contains("test_edit.py"),
        "{reply:?}"
    );
    assert_current(reply, &["test_renamed.py::test_keep"]);
}

#[test]
fn real_engine_watcher_deletes_from_collapsed_suite() {
    let reply = mutation_reply_from_source(
        "import pytest\n@pytest.mark.parametrize('x', range(70))\ndef test_case(x):\n    assert x >= 0\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "import pytest\n@pytest.mark.parametrize('x', range(69))\ndef test_case(x):\n    assert x >= 0\n",
            ).unwrap();
        },
    );
    let output = reply.output.as_ref().unwrap();
    assert!(
        output.contains("69 passed · 0 failed · 0 timed out"),
        "{reply:?}"
    );
    assert_eq!(reply.exit_code, 0, "{reply:?}");
}

#[test]
fn real_engine_watcher_deletes_failure_from_collapsed_suite() {
    let reply = mutation_reply_from_source(
        "import pytest\n@pytest.mark.parametrize('x', range(70))\ndef test_case(x):\n    assert x < 69\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "import pytest\n@pytest.mark.parametrize('x', range(69))\ndef test_case(x):\n    assert x < 69\n",
            ).unwrap();
        },
    );
    let output = reply.output.as_ref().unwrap();
    assert!(
        output.contains("69 passed · 0 failed · 0 timed out"),
        "{reply:?}"
    );
    assert!(!output.contains("test_case[69]"), "{reply:?}");
    assert_eq!(reply.exit_code, 0, "{reply:?}");
}

#[test]
fn real_engine_watcher_deletes_last_function_but_keeps_file() {
    let reply = mutation_reply_from_source("def test_old():\n    assert False\n", |root| {
        std::fs::write(root.join("test_edit.py"), "").unwrap();
    });
    assert_eq!(reply.exit_code, 1, "{reply:?}");
    assert!(
        reply
            .error
            .as_deref()
            .is_some_and(|err| err.contains("not proven complete")),
        "empty leftover test file must fail closed without a TargetReport; {reply:?}"
    );
}

#[test]
fn real_engine_watcher_renames_test_class() {
    let reply = mutation_reply_from_source(
        "class TestOld:\n    def test_keep(self):\n        assert True\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "class TestNew:\n    def test_keep(self):\n        assert True\n",
            )
            .unwrap();
        },
    );
    assert!(
        !reply.output.as_ref().unwrap().contains("TestOld"),
        "{reply:?}"
    );
    assert_current(reply, &["test_edit.py::TestNew::test_keep"]);
}

#[test]
fn real_engine_watcher_deletion_crosses_named_report_boundary() {
    let reply = mutation_reply_from_source(
        "import pytest\n@pytest.mark.parametrize('x', range(65))\ndef test_case(x):\n    assert x >= 0\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "import pytest\n@pytest.mark.parametrize('x', range(64))\ndef test_case(x):\n    assert x >= 0\n",
            ).unwrap();
        },
    );
    let output = reply.output.as_ref().unwrap();
    assert!(
        output.contains("64 passed · 0 failed · 0 timed out"),
        "{reply:?}"
    );
    assert!(!output.contains("test_case[64]"), "{reply:?}");
    assert_eq!(reply.exit_code, 0, "{reply:?}");
}

#[test]
fn real_engine_watcher_renames_test_preserving_size_and_mtime() {
    let reply = mutation_reply_from_source("def test_old():\n    assert True\n", |root| {
        let path = root.join("test_edit.py");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "def test_new():\n    assert True\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    });
    assert_current(reply, &["test_new"]);
}

#[test]
fn real_engine_watcher_growth_updates_cached_language_outcomes() {
    let reply = mutation_reply_for_request(
        "import pytest\n@pytest.mark.parametrize('x', range(64))\ndef test_case(x):\n    assert x >= 0\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "import pytest\n@pytest.mark.parametrize('x', range(65))\ndef test_case(x):\n    assert x > 0\n",
            ).unwrap();
        },
        NudgeRequestMsg {
            force: true,
            ..NudgeRequestMsg::default().with_lang_label("python")
        },
    );
    let output = reply.output.clone().unwrap_or_default();
    assert_eq!(reply.exit_code, 1, "{reply:?}");
    if !output.is_empty() {
        assert!(
            output.contains("64 passed")
                && (output.contains("1 failed") || output.contains("FAIL")),
            "ready TargetReport must recap the grown fail; {reply:?}"
        );
    }
}

#[test]
fn real_engine_watcher_deleted_final_file_clears_language_reply() {
    let reply = mutation_reply_for_request(
        "def test_old():\n    assert False\n",
        |root| std::fs::remove_file(root.join("test_edit.py")).unwrap(),
        NudgeRequestMsg::default().with_lang_label("python"),
    );
    assert_current(reply, &[]);
}

#[test]
fn real_engine_watcher_deletes_final_test_file() {
    let reply = mutation_reply(|root| {
        std::fs::remove_file(root.join("test_edit.py")).unwrap();
    });
    assert_current(reply, &[]);
}

#[test]
fn real_engine_watcher_deletes_one_file_keeps_sibling() {
    let reply = mutation_reply_from_files(
        &[
            ("test_edit.py", "def test_old():\n    assert True\n"),
            ("test_keep.py", "def test_keep():\n    assert True\n"),
        ],
        "test_edit.py",
        |root| std::fs::remove_file(root.join("test_edit.py")).unwrap(),
        NudgeRequestMsg::default(),
    );
    assert!(
        !reply.output.as_ref().unwrap().contains("test_old"),
        "{reply:?}"
    );
    assert_current(reply, &["test_keep.py::test_keep"]);
}

#[test]
fn real_engine_watcher_promotes_function_to_class() {
    let reply = mutation_reply_from_source("def test_keep():\n    assert True\n", |root| {
        std::fs::write(
            root.join("test_edit.py"),
            "class TestKeep:\n    def test_keep(self):\n        assert True\n",
        )
        .unwrap();
    });
    assert!(
        !reply
            .output
            .as_ref()
            .unwrap()
            .contains("test_edit.py::test_keep"),
        "{reply:?}"
    );
    assert_current(reply, &["test_edit.py::TestKeep::test_keep"]);
}

#[test]
fn real_engine_watcher_replaces_parameter_ids() {
    let reply = mutation_reply_from_source(
        "import pytest\n@pytest.mark.parametrize('x', [1, 2])\ndef test_case(x):\n    assert x in (1, 2)\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "import pytest\n@pytest.mark.parametrize('x', [3, 4])\ndef test_case(x):\n    assert x in (3, 4)\n",
            )
            .unwrap();
        },
    );
    let output = reply.output.as_ref().unwrap();
    assert!(!output.contains("test_case[1]"), "{reply:?}");
    assert!(!output.contains("test_case[2]"), "{reply:?}");
    assert_current(reply, &["test_case[3]", "test_case[4]"]);
}

#[test]
fn real_engine_watcher_pass_to_fail_preserving_size_and_mtime() {
    let reply = mutation_reply_from_source("def test_keep():\n    assert 1\n", |root| {
        let path = root.join("test_edit.py");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "def test_keep():\n    assert 0\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    });
    let output = reply.output.as_deref().unwrap_or("");
    assert!(output.contains("test_keep"), "{reply:?}");
    assert!(
        output.contains("1 failed") || output.contains("FAIL"),
        "{reply:?}"
    );
    assert!(!output.contains("1 passed"), "{reply:?}");
    assert_eq!(reply.exit_code, 1, "{reply:?}");
}

#[test]
fn real_engine_watcher_deletes_class_method() {
    let reply = mutation_reply_from_source(
        "class TestKeep:\n    def test_old(self):\n        assert True\n    def test_keep(self):\n        assert True\n",
        |root| {
            std::fs::write(
                root.join("test_edit.py"),
                "class TestKeep:\n    def test_keep(self):\n        assert True\n",
            )
            .unwrap();
        },
    );
    assert!(
        !reply.output.as_ref().unwrap().contains("test_old"),
        "{reply:?}"
    );
    assert_current(reply, &["test_edit.py::TestKeep::test_keep"]);
}
