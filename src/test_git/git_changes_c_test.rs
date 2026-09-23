use std::collections::BTreeSet;
use std::fs;

use tempfile::TempDir;

use super::*;

fn git_in(dir: &std::path::Path) -> std::process::Command {
    super::git_command(dir)
}

fn init_repo(tmp: &TempDir) {
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

fn commit_then_edit(tmp: &TempDir) {
    fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "m"])
            .status()
            .unwrap()
            .success()
    );
    fs::write(tmp.path().join("a.py"), "x=2\n").unwrap();
}

#[test]
fn changed_lines_commit_survives_color_always() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_then_edit(&tmp);
    assert!(
        git_in(tmp.path())
            .args(["config", "color.ui", "always"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["config", "color.diff", "always"])
            .status()
            .unwrap()
            .success()
    );
    let lines = changed_lines_commit(tmp.path()).unwrap();
    assert_eq!(
        lines.get("a.py"),
        Some(&BTreeSet::from([1])),
        "commit: color.ui=always must not drop line maps, got {lines:?}"
    );
}

#[test]
fn changed_lines_commit_survives_noprefix() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_then_edit(&tmp);
    assert!(
        git_in(tmp.path())
            .args(["config", "diff.noprefix", "true"])
            .status()
            .unwrap()
            .success()
    );
    let lines = changed_lines_commit(tmp.path()).unwrap();
    assert_eq!(
        lines.get("a.py"),
        Some(&BTreeSet::from([1])),
        "commit: diff.noprefix must not drop line maps, got {lines:?}"
    );
}

#[test]
fn changed_lines_commit_survives_external_diff() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_then_edit(&tmp);
    assert!(
        git_in(tmp.path())
            .args(["config", "diff.external", "echo"])
            .status()
            .unwrap()
            .success()
    );
    let lines = changed_lines_commit(tmp.path()).unwrap();
    assert_eq!(
        lines.get("a.py"),
        Some(&BTreeSet::from([1])),
        "commit: diff.external must not drop line maps, got {lines:?}"
    );
}

#[test]
fn changed_lines_commit_uses_ondisk_line_numbers_with_textconv() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    fs::write(tmp.path().join(".gitattributes"), "*.py diff=py\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["config", "diff.py.textconv", "sed \"1i HDR\""])
            .status()
            .unwrap()
            .success()
    );
    fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py", ".gitattributes"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "m"])
            .status()
            .unwrap()
            .success()
    );
    fs::write(tmp.path().join("a.py"), "x=2\n").unwrap();
    let lines = changed_lines_commit(tmp.path()).unwrap();
    assert_eq!(
        lines.get("a.py"),
        Some(&BTreeSet::from([1])),
        "commit: textconv must not shift on-disk line numbers, got {lines:?}"
    );
}

#[test]
fn changed_lines_commit_keeps_hunk_after_triple_plus_content() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    fs::write(tmp.path().join("a.py"), "line1\nline2\nline3\nline4\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "m"])
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        tmp.path().join("a.py"),
        "line1\n++ b/evil.py\nline3\nCHANGED\n",
    )
    .unwrap();
    let lines = changed_lines_commit(tmp.path()).unwrap();
    assert_eq!(
        lines.get("a.py"),
        Some(&BTreeSet::from([2, 4])),
        "commit: ++ content line must not steal later hunks, got {lines:?}"
    );
    assert!(
        !lines.contains_key("evil.py"),
        "commit: spoof +++ path must not become a file key, got {lines:?}"
    );
}

#[test]
fn changed_lines_commit_survives_mnemonic_prefix() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_then_edit(&tmp);
    assert!(
        git_in(tmp.path())
            .args(["config", "diff.mnemonicPrefix", "true"])
            .status()
            .unwrap()
            .success()
    );
    let lines = changed_lines_commit(tmp.path()).unwrap();
    assert_eq!(
        lines.get("a.py"),
        Some(&BTreeSet::from([1])),
        "commit: diff.mnemonicPrefix must not drop line maps, got {lines:?}"
    );
}
