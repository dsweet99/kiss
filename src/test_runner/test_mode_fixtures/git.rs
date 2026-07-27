use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

pub(crate) fn git_in(dir: &Path) -> Command {
    crate::test_git::git_command(dir)
}

pub(crate) fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = git_in(dir).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub(crate) fn init_git(tmp: &TempDir) {
    assert!(
        git_in(tmp.path())
            .args(["init", "-b", "main"])
            .status()
            .unwrap()
            .success()
    );
    git_in(tmp.path())
        .args(["config", "user.email", "t@t.t"])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["config", "user.name", "t"])
        .status()
        .unwrap();
}

pub(crate) fn ensure_main_branch(dir: &Path) {
    let out = git_in(dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .unwrap();
    if !out.status.success() {
        // Empty repo: `git init -b main` already set the initial branch name.
        return;
    }
    let current = String::from_utf8(out.stdout).unwrap().trim().to_string();
    if current != "main" {
        assert!(
            git_in(dir)
                .args(["branch", "-M", "main"])
                .status()
                .unwrap()
                .success()
        );
    }
}

pub(crate) fn with_cwd<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let out = f();
    std::env::set_current_dir(orig).unwrap();
    out
}

pub(crate) fn checkout_branch(dir: &Path, name: &str) {
    assert!(
        git_in(dir)
            .args(["checkout", "-b", name])
            .status()
            .unwrap()
            .success()
    );
}

pub(crate) fn commit_all(dir: &Path, message: &str) {
    assert!(
        git_in(dir)
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(dir)
            .args(["commit", "-m", message])
            .status()
            .unwrap()
            .success()
    );
}
