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

fn commit_file(tmp: &TempDir, rel: &str, body: &str, message: &str) {
    if let Some(parent) = std::path::Path::new(rel).parent() {
        fs::create_dir_all(tmp.path().join(parent)).unwrap();
    }
    fs::write(tmp.path().join(rel), body).unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", rel])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", message])
            .status()
            .unwrap()
            .success()
    );
}

fn rev_parse(tmp: &TempDir, rev: &str) -> String {
    let out = git_in(tmp.path())
        .args(["rev-parse", rev])
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn resolve_diff_target_base_with_branch_returns_merge_base_sha() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    assert!(
        git_in(tmp.path())
            .args(["checkout", "-b", "feature"])
            .status()
            .unwrap()
            .success()
    );
    commit_file(&tmp, "b.py", "y=1\n", "feat");
    let expected = merge_base(tmp.path(), "main").unwrap();
    let got = resolve_diff_target(tmp.path(), TestChangeMode::Base, None, None, Some("main"))
        .unwrap()
        .expect("base target");
    assert_eq!(
        got, expected,
        "base: explicit --base-branch must resolve to merge-base SHA"
    );
    assert_eq!(got.len(), 40, "base: merge-base must be a full SHA");
}

#[test]
fn resolve_diff_target_base_none_auto_detects_fork_sha() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    assert!(
        git_in(tmp.path())
            .args(["checkout", "-b", "feature"])
            .status()
            .unwrap()
            .success()
    );
    commit_file(&tmp, "b.py", "y=1\n", "feat");
    let auto = auto_detect_fork_commit(tmp.path()).unwrap();
    let got = resolve_diff_target(tmp.path(), TestChangeMode::Base, None, None, None)
        .unwrap()
        .expect("auto base");
    assert_eq!(
        got, auto,
        "base: auto-detect must return auto_detect_fork_commit SHA"
    );
}

#[test]
fn resolve_diff_target_base_none_single_branch_errors() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    let err = resolve_diff_target(tmp.path(), TestChangeMode::Base, None, None, None)
        .expect_err("single-branch base");
    assert!(
        err.contains("--base-branch"),
        "base: single-branch error must mention --base-branch, got {err}"
    );
}

#[test]
fn resolve_diff_target_main_returns_ref_name_not_merge_base() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    assert!(
        git_in(tmp.path())
            .args(["checkout", "-b", "feature"])
            .status()
            .unwrap()
            .success()
    );
    commit_file(&tmp, "b.py", "y=1\n", "feat");
    let got = resolve_diff_target(tmp.path(), TestChangeMode::Main, None, Some("main"), None)
        .unwrap()
        .expect("main target");
    assert_eq!(got, "main", "main: must return ref name, not merge-base SHA");
    assert_ne!(got, merge_base(tmp.path(), "main").unwrap());
    let via_cfg = resolve_diff_target(tmp.path(), TestChangeMode::Main, Some("main"), None, None)
        .unwrap()
        .expect("main config target");
    assert_eq!(
        via_cfg, "main",
        "main: config main_branch must resolve to the same local ref name"
    );
}

#[test]
fn resolve_main_branch_fallback_order_prefers_origin_then_local() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    let sha = rev_parse(&tmp, "HEAD");
    assert!(
        git_in(tmp.path())
            .args(["update-ref", "refs/remotes/origin/trunk", &sha])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["branch", "-M", "feature"])
            .status()
            .unwrap()
            .success()
    );
    let name = resolve_main_branch_name(tmp.path(), None, Some("trunk")).unwrap();
    assert_eq!(name, "origin/trunk", "main: prefer origin/<name> first");
}

#[test]
fn resolve_main_branch_fallback_uses_local_name_then_master() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    // No origin/* refs: local `<name>` wins over master candidates.
    let local = resolve_main_branch_name(tmp.path(), Some("main"), None).unwrap();
    assert_eq!(local, "main", "main: local <name> when origin/<name> absent");

    assert!(
        git_in(tmp.path())
            .args(["branch", "-M", "feature"])
            .status()
            .unwrap()
            .success()
    );
    let sha = rev_parse(&tmp, "HEAD");
    assert!(
        git_in(tmp.path())
            .args(["update-ref", "refs/remotes/origin/master", &sha])
            .status()
            .unwrap()
            .success()
    );
    // Missing origin/<cli-name> and local <cli-name>: fall through to origin/master.
    let via_master = resolve_main_branch_name(tmp.path(), None, Some("does-not-exist")).unwrap();
    assert_eq!(
        via_master, "origin/master",
        "main: origin/master after missing origin/<name> and <name>"
    );
}

#[test]
fn untracked_asymmetry_commit_includes_since_excludes() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    commit_file(&tmp, "a.py", "x=1\n", "root");
    fs::write(tmp.path().join("untracked.py"), "z=1\n").unwrap();
    let commit_paths = changed_paths_commit(tmp.path()).unwrap();
    assert!(
        commit_paths.iter().any(|n| n.ends_with("untracked.py")),
        "commit: must include untracked, got {commit_paths:?}"
    );
    let mb = merge_base(tmp.path(), "HEAD").unwrap();
    let since = changed_paths_since(tmp.path(), &mb).unwrap();
    assert!(
        !since.iter().any(|n| n.ends_with("untracked.py")),
        "since: must not include untracked, got {since:?}"
    );
}

#[test]
fn changed_lines_since_reports_new_line_numbers() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "one\ntwo\nthree\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "."])
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
    let baseline = rev_parse(&tmp, "HEAD");
    fs::write(
        tmp.path().join("src").join("lib.rs"),
        "one\ntwo changed\nthree\nfour\n",
    )
    .unwrap();
    let lines = changed_lines_since(tmp.path(), &baseline).unwrap();
    assert_eq!(
        lines["src/lib.rs"],
        BTreeSet::from([2, 4]),
        "changed_lines_since must report new line numbers"
    );
}
