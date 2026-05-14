use std::path::Path;

use crate::support::git::git_command;

fn init_git_repo(dir: &Path) {
    assert!(
        git_command(dir)
            .args(["init"])
            .status()
            .unwrap()
            .success()
    );
    for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
        git_command(dir)
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
        git_command(tmp.path())
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_command(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(tmp.path().join("lib.py"), "def f():\n    return 1\n").unwrap();
    let out = std::process::Command::new(bin)
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
