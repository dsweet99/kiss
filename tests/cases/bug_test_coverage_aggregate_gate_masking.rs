//! Regression for per-file test-coverage gate enforcement in production `kiss check`.
//!
//! `KPop` bughunt (`_malvin/20260523_185300_52l1g7oq/_kpop/exp_log_*.md`): whole-repo
//! `kiss check` must fail when any production file is below the threshold, even if
//! aggregate coverage clears the gate (18/19 referenced with threshold 90).

use std::fmt::Write as _;
use std::fs;
use std::process::Command;
use crate::support::kiss_test::kiss_command;
use tempfile::TempDir;

const COVERED_FUNCTION_COUNT: usize = 18;

fn kiss_binary() -> Command {
    kiss_command()
}

fn write_permissive_config(root: &std::path::Path) {
    fs::write(
        root.join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 90\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
         \n\
         [python]\n\
         functions_per_file = 100\n\
         statements_per_file = 1000\n\
         lines_per_file = 1000\n\
         imported_names_per_file = 100\n",
    )
    .unwrap();
}

fn write_aggregate_masking_corpus(root: &std::path::Path) {
    let mut good = String::from("import bad\n\n");
    for i in 1..=COVERED_FUNCTION_COUNT {
        write!(good, "def f{i}():\n    return {i}\n\n").unwrap();
    }
    fs::write(root.join("good.py"), good).unwrap();
    fs::write(root.join("bad.py"), "def orphan_func():\n    pass\n").unwrap();

    let imports: String = (1..=COVERED_FUNCTION_COUNT)
        .map(|i| format!("f{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let calls: String = (1..=COVERED_FUNCTION_COUNT)
        .map(|i| format!("f{i}()"))
        .collect::<Vec<_>>()
        .join("; ");
    fs::write(
        root.join("test_good.py"),
        format!("from good import {imports}\n\ndef test_all():\n    {calls}\n"),
    )
    .unwrap();
    write_permissive_config(root);
}

fn run_check_from_corpus_root(
    home: &std::path::Path,
    root: &std::path::Path,
    target: &str,
) -> std::process::Output {
    kiss_binary()
        .current_dir(root)
        .env("HOME", home)
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(target)
        .output()
        .expect("kiss check should run")
}

/// Whole-repo check must fail when any production file is below the threshold,
/// even if aggregate coverage clears the gate (18/19 referenced with threshold 90).
#[test]
fn bug_whole_repo_check_fails_when_one_file_below_coverage_threshold() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);

    let out = run_check_from_corpus_root(home.path(), root, ".");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_ne!(
        out.status.code(),
        Some(0),
        "whole-repo check must fail when bad.py has 0% coverage despite aggregate ≥ 90%.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("VIOLATION:test_coverage"),
        "expected test_coverage gate failure on whole-repo check.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("bad.py"),
        "failure output should name bad.py.\nstdout:\n{stdout}"
    );
}

/// Focused and whole-repo checks must agree on test-coverage pass/fail for the same file.
#[test]
fn bug_whole_repo_and_focused_check_agree_on_coverage_gate() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);

    let focused = run_check_from_corpus_root(home.path(), root, "bad.py");
    let whole = run_check_from_corpus_root(home.path(), root, ".");

    let focused_stdout = String::from_utf8_lossy(&focused.stdout);
    let whole_stdout = String::from_utf8_lossy(&whole.stdout);

    assert_eq!(
        focused.status.code(),
        whole.status.code(),
        "focused and whole-repo checks must agree on exit status.\n\
         focused stdout:\n{focused_stdout}\n\
         whole stdout:\n{whole_stdout}"
    );
}
