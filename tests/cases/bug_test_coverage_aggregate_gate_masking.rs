use crate::common::seed_python_runtime_coverage;
use crate::support::git::{commit_all, init_git_repo};
use std::fmt::Write as _;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

const COVERED_FUNCTION_COUNT: usize = 10;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_permissive_config(root: &std::path::Path) {
    write_permissive_config_with_scope(root, None);
}

fn write_permissive_config_with_scope(root: &std::path::Path, scope: Option<&str>) {
    let scope_line = scope.map_or(String::new(), |s| {
        format!("test_coverage_scope = \"{s}\"\n")
    });
    fs::write(
        root.join(".kissconfig"),
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             \n\
             [test]\n\
             test_coverage_threshold = 90\n\
             {scope_line}\
             \n\
             [python]\n\
             functions_per_file = 100\n\
             statements_per_file = 1000\n\
             lines_per_file = 1000\n\
             imported_names_per_file = 100\n"
        ),
    )
    .unwrap();
}

fn write_aggregate_masking_corpus(root: &std::path::Path) {
    init_git_repo(root);
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
    seed_python_runtime_coverage(
        root,
        &[(
            "test_good.py::test_all",
            // Cover every line of good.py; size scales with COVERED_FUNCTION_COUNT.
            vec![(
                "good.py",
                (1..=(fs::read_to_string(root.join("good.py"))
                    .unwrap()
                    .lines()
                    .count()
                    .max(1) as u32))
                    .collect::<Vec<_>>(),
            )],
        )],
    );
    write_permissive_config(root);
    commit_all(root, "init");
}

fn run_cov_from_corpus_root(
    home: &std::path::Path,
    root: &std::path::Path,
    target: &str,
) -> std::process::Output {
    kiss_binary()
        .current_dir(root)
        .env("HOME", home)
        .arg("test")
        .arg("--lang")
        .arg("python")
        .arg(target)
        .output()
        .expect("kiss test should run")
}

#[test]
fn bug_whole_repo_check_fails_when_one_file_below_coverage_threshold() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);
    write_permissive_config_with_scope(root, Some("by_file"));

    let out = run_cov_from_corpus_root(home.path(), root, ".");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_ne!(
        out.status.code(),
        Some(0),
        "by_file whole-repo cov must fail when bad.py has 0% coverage despite aggregate ≥ 90%.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("VIOLATION:test_coverage"),
        "expected test_coverage violation on whole-repo cov.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("bad.py"),
        "failure output should name bad.py.\nstdout:\n{stdout}"
    );
}

#[test]
fn bug_whole_repo_and_focused_check_agree_on_coverage_gate() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);
    write_permissive_config_with_scope(root, Some("by_file"));

    // Focused path is the expensive agreement witness; whole-repo fail is covered by the
    // sibling test. One kiss subprocess keeps this under the unit-test SLA.
    let focused = run_cov_from_corpus_root(home.path(), root, "bad.py");
    let focused_stdout = String::from_utf8_lossy(&focused.stdout);
    let focused_stderr = String::from_utf8_lossy(&focused.stderr);

    assert_ne!(
        focused.status.code(),
        Some(0),
        "focused bad.py must fail under by_file scope (same gate as whole-repo).\n\
         stdout:\n{focused_stdout}\nstderr:\n{focused_stderr}"
    );
    assert!(
        focused_stdout.contains("VIOLATION:test_coverage") || focused_stdout.contains("bad.py"),
        "focused failure must name the coverage gate / bad.py.\nstdout:\n{focused_stdout}"
    );
}

#[test]
fn default_scope_whole_repo_passes_when_aggregate_clears() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);

    let out = run_cov_from_corpus_root(home.path(), root, ".");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(0),
        "default/omitted scope whole-repo cov must pass when aggregate ≥ 90%.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("per-file enforcement"),
        "default codebase path must not emit per-file enforcement.\nstdout:\n{stdout}"
    );
}

#[test]
fn codebase_scope_whole_repo_passes_when_aggregate_clears() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);
    write_permissive_config_with_scope(root, Some("codebase"));

    let out = run_cov_from_corpus_root(home.path(), root, ".");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(0),
        "codebase scope whole-repo cov must pass when aggregate ≥ 90%.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("per-file enforcement"),
        "codebase pass path must not emit per-file enforcement.\nstdout:\n{stdout}"
    );
}

#[test]
fn codebase_scope_focused_bad_py_fails() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = tmp.path();
    write_aggregate_masking_corpus(root);
    write_permissive_config_with_scope(root, Some("codebase"));

    let out = run_cov_from_corpus_root(home.path(), root, "bad.py");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_ne!(
        out.status.code(),
        Some(0),
        "codebase scope focused on bad.py must fail.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("codebase coverage"),
        "expected codebase gate failure header.\nstdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("per-file enforcement"),
        "codebase failure must not use per-file enforcement wording.\nstdout:\n{stdout}"
    );
}
