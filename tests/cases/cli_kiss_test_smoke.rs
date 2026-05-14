use std::path::Path;
use std::process::Command;

// Scrub `GIT_*` env vars on every git invocation. When this test runs
// under pre-commit, git sets `GIT_INDEX_FILE` to the parent repo's
// `.git/index.lock`; without the scrub, every `git add`/`git commit`
// here writes index entries into the parent repo's lock file while the
// blobs go to the tempdir's `.git/objects`. The tempdir is dropped on
// test end, leaving a phantom blob OID in the parent's lock file that
// makes the user's outer commit fail with "invalid object ... Error
// building trees".
fn git_in(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir)
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_COMMON_DIR");
    c
}

fn init_git_repo(dir: &Path) {
    assert!(
        git_in(dir)
            .args(["init"])
            .status()
            .unwrap()
            .success()
    );
    for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
        git_in(dir)
            .args(["config", kv.0, kv.1])
            .status()
            .unwrap();
    }
}

#[test]
fn kiss_test_dry_run_in_git_repo_smoke() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bin = env!("CARGO_BIN_EXE_kiss");
    init_git_repo(tmp.path());
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    pass\n").unwrap();
    std::fs::write(
        tmp.path().join("test_lib.py"),
        "from lib import f\ndef test_f():\n    f()\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 1\n").unwrap();
    let out = Command::new(bin)
        .current_dir(tmp.path())
        .args(["test", "commit", "--dry-run"])
        .output()
        .expect("kiss test");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "kiss test --dry-run should exit 0, stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("pytest") || stdout.contains("NO COVERING"),
        "unexpected stdout: {stdout}"
    );
}
