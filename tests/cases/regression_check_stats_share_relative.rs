use crate::common::{list_full_check_cache_files, seed_python_runtime_coverage};
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
    seed_python_runtime_coverage(
        repo.path(),
        &[(
            "test_share.py::test_covered_function",
            vec![("share.py", vec![1, 2])],
        )],
    );

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
    let after_check = list_full_check_cache_files(repo.path());
    assert_eq!(
        after_check.len(),
        1,
        "expected exactly one cache file after `kiss check .`; got {after_check:?}"
    );
    let _stats = run("stats");
    let after_stats = list_full_check_cache_files(repo.path());
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
