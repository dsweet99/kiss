//! Regression test for `kiss check --ignore=PREFIX` filename-matching bug.
//!
//! Bug discovered via KPOP (see `_kpop/exp_log_kiss_check_bug.md`):
//! The `--ignore=<PREFIX>` flag's help text in `src/bin_cli/args.rs` reads
//! `"Ignore files/directories starting with PREFIX (repeatable)"`, but the
//! implementation in `src/discovery/mod.rs::should_ignore` only matches
//! directory components and explicitly skips the filename. This means
//! `kiss check --ignore=big <dir>` does NOT ignore a file named `big.py`,
//! contradicting the documented behavior.
//!
//! These tests assert the contract documented in the help text. They are
//! gated with `#[ignore]` so CI stays green until the bug is fixed; once
//! the implementation is updated to honor the documented contract, the
//! `#[ignore]` attributes can be removed.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_trivial_py(path: &std::path::Path) {
    fs::write(path, "def f():\n    return 1\n").unwrap();
}

/// Baseline (locks in current behavior): `--ignore` correctly excludes a
/// directory whose name starts with PREFIX. This part of the contract
/// already works and should never regress.
#[test]
fn cli_check_ignore_excludes_matching_directory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("subdir")).unwrap();
    write_trivial_py(&root.join("keep.py"));
    write_trivial_py(&root.join("subdir").join("drop.py"));

    let baseline = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg(root)
        .output()
        .unwrap();
    let baseline_stdout = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        baseline_stdout.contains("Analyzed: 2 files"),
        "baseline should see both files. stdout:\n{baseline_stdout}"
    );

    let filtered = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg("--ignore=subdir")
        .arg(root)
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(
        filtered_stdout.contains("Analyzed: 1 files"),
        "--ignore=subdir should drop the subdir file. stdout:\n{filtered_stdout}"
    );
}

/// Regression test for the documented-but-broken behavior:
/// `--ignore=PREFIX` should also exclude files whose names start with
/// PREFIX, per the `--ignore` help text in `src/bin_cli/args.rs`.
///
/// Currently FAILS because `src/discovery/mod.rs::should_ignore` only
/// inspects directory components. Marked `#[ignore]` so this test can land
/// alongside the bug report without breaking CI; un-ignore once
/// `should_ignore` (and the test in `src/discovery/discovery_test.rs`
/// asserting the opposite) are updated.
#[test]
#[ignore = "Bug: --ignore=PREFIX does not match filenames; see _kpop/exp_log_kiss_check_bug.md"]
fn cli_check_ignore_excludes_matching_filename() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_trivial_py(&root.join("big.py"));
    write_trivial_py(&root.join("small.py"));

    let baseline = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg(root)
        .output()
        .unwrap();
    let baseline_stdout = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        baseline_stdout.contains("Analyzed: 2 files"),
        "baseline should see both files. stdout:\n{baseline_stdout}"
    );

    let filtered = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg("--ignore=big")
        .arg(root)
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    let filtered_stderr = String::from_utf8_lossy(&filtered.stderr);
    assert!(
        filtered_stdout.contains("Analyzed: 1 files"),
        "--ignore=big should drop big.py per documented behavior \
         (\"Ignore files/directories starting with PREFIX\"). \
         stdout:\n{filtered_stdout}\nstderr:\n{filtered_stderr}"
    );
    assert!(
        !filtered_stdout.contains("big.py"),
        "big.py should not appear in violations after --ignore=big. \
         stdout:\n{filtered_stdout}"
    );
}
