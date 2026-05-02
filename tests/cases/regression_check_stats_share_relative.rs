//! Regression for `kiss check` and `kiss stats` not sharing the analyze
//! cache when invoked with a *relative* path argument (the default).
//!
//! Before the fix, `kiss check` canonicalized discovered file paths
//! (`src/analyze/focus.rs::gather_files`) while `kiss stats`'s
//! `collect_files` did not, so the two commands hashed different path
//! strings into `fingerprint_for_check` and produced two separate
//! `check_full_*.bin` files when the input root was relative (`.`).
//! After the fix (use `kiss::discovery::gather_files_by_lang` in stats),
//! both commands produce the same fingerprint and share one cache file.
//! See `_kpop/exp_log_check_stats_share.md` (round 2).

use crate::common::list_full_check_cache_files;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn regression_check_stats_share_cache_with_relative_path() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let src = repo.path().join("share.py");
    let test = repo.path().join("test_share.py");
    fs::write(&src, "def covered_function(x):\n    return x * 2\n").unwrap();
    fs::write(
        &test,
        "from share import covered_function\n\ndef test_covered_function():\n    assert covered_function(2) == 4\n",
    )
    .unwrap();

    let run = |cmd: &str| {
        kiss_binary()
            .current_dir(repo.path())
            .arg("--defaults")
            .arg(cmd)
            .arg("--lang")
            .arg("python")
            .arg(".")
            .env("HOME", home.path())
            .output()
            .unwrap()
    };

    let _check = run("check");
    let after_check = list_full_check_cache_files(home.path());
    assert_eq!(
        after_check.len(),
        1,
        "expected exactly one cache file after `kiss check .`; got {after_check:?}"
    );
    let _stats = run("stats");
    let after_stats = list_full_check_cache_files(home.path());
    assert_eq!(
        after_stats.len(),
        1,
        "after `kiss stats .` the cache dir should still contain a single file (shared with check); got {after_stats:?}"
    );
    assert_eq!(
        after_check[0].file_name(),
        after_stats[0].file_name(),
        "fingerprint must match between `kiss check .` and `kiss stats .` so they share the cache"
    );
}
