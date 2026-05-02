//! Regression test for `kiss check` focus filter being silently disabled
//! when the focus path resolves to zero source files.
//!
//! Bug discovered via KPOP (see `_kpop/exp_log_kiss_check_bug2.md`):
//! `kiss check UNIVERSE FOCUS` is documented as "report only these"
//! (`src/bin_cli/args.rs:43`). When `FOCUS` is a directory containing no
//! `.py` / `.rs` files (or whose source files are all filtered out by
//! `--lang` / `--ignore`), `build_focus_set` (`src/analyze/focus.rs:26`)
//! returns an empty `HashSet`. Then `is_focus_file`
//! (`src/analyze/focus.rs:47`) treats the empty set as "no filter active"
//! and returns `true`, so `filter_viols_by_focus` retains every
//! universe-wide violation. The user sees the full report instead of the
//! narrowed/empty one they asked for.
//!
//! Tests are gated with `#[ignore]` so CI stays green until the bug is
//! fixed; remove `#[ignore]` once `is_focus_file` distinguishes
//! "no focus specified" from "focus specified but matched zero files".

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

/// Writes a file that triggers a `positional_args` violation under defaults
/// (threshold is 3 positional args; this function has 8).
fn write_violating_py(path: &std::path::Path) {
    fs::write(path, "def big(a, b, c, d, e, f, g, h):\n    return a\n").unwrap();
}

/// Baseline (locks in current correct behavior): when the focus path is a
/// directory that *does* contain a source file, focus filtering correctly
/// restricts violations to that file.
#[test]
fn cli_check_focus_dir_with_source_restricts_report() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("focus_dir")).unwrap();
    write_violating_py(&root.join("src").join("big.py"));
    fs::write(
        root.join("focus_dir").join("ok.py"),
        "def g(x):\n    return x\n",
    )
    .unwrap();

    let out = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg("--defaults")
        .arg(root)
        .arg(root.join("focus_dir"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("big.py"),
        "focus=focus_dir/ should hide src/big.py violations. stdout:\n{stdout}"
    );
}

/// Regression test for the bug: when the focus path is a directory with
/// NO source files, `kiss check` should report nothing for those focus
/// paths (or warn) — instead it silently dumps every universe violation.
///
/// Currently FAILS because `is_focus_file` treats an empty `focus_set`
/// as "no filter".
#[test]
fn cli_check_focus_dir_with_no_source_does_not_leak_universe() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("non_src")).unwrap();
    write_violating_py(&root.join("src").join("big.py"));
    fs::write(root.join("non_src").join("readme.txt"), "hello\n").unwrap();

    let universe_only = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg("--defaults")
        .arg(root)
        .output()
        .unwrap();
    let universe_stdout = String::from_utf8_lossy(&universe_only.stdout);
    assert!(
        universe_stdout.contains("big.py"),
        "sanity: universe-only run should report big.py. stdout:\n{universe_stdout}"
    );

    let focused = kiss_binary()
        .arg("check")
        .arg("--all")
        .arg("--defaults")
        .arg(root)
        .arg(root.join("non_src"))
        .output()
        .unwrap();
    let focused_stdout = String::from_utf8_lossy(&focused.stdout);
    let focused_stderr = String::from_utf8_lossy(&focused.stderr);
    assert!(
        !focused_stdout.contains("big.py"),
        "focus=non_src/ (no source files) must not leak src/big.py violations. \
         stdout:\n{focused_stdout}\nstderr:\n{focused_stderr}"
    );
}
