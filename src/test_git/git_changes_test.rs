use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::*;

// Use the production scrubbed-env builder so these tests don't pick
// up `GIT_INDEX_FILE`/`GIT_DIR` from an outer pre-commit wrapper and
// silently operate on the real repo's index.
fn git_in(dir: &Path) -> Command {
    super::git_command(dir)
}

fn init_repo(tmp: &TempDir) {
    let status = git_in(tmp.path())
        .args(["init"])
        .status()
        .expect("git init");
    assert!(status.success());
    git_in(tmp.path())
        .args(["config", "user.email", "t@t.t"])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["config", "user.name", "t"])
        .status()
        .unwrap();
}

#[test]
fn assert_git_repo_rejects_without_git() {
    let tmp = TempDir::new().unwrap();
    assert!(assert_git_repo(tmp.path()).is_err());
}

#[test]
fn repo_root_and_commit_changed_paths() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    assert_git_repo(tmp.path()).unwrap();
    let root = git_repo_root(tmp.path()).unwrap();
    assert_eq!(root, tmp.path().canonicalize().unwrap());
    std::fs::write(tmp.path().join("b.py"), "y=1\n").unwrap();
    let names = changed_paths_commit(tmp.path()).unwrap();
    assert!(names.iter().any(|n| n.ends_with("b.py")));
}

#[test]
fn diff_filter_drops_deleted_file() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("keep.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    std::fs::remove_file(tmp.path().join("keep.py")).unwrap();
    let names = changed_paths_commit(tmp.path()).unwrap();
    assert!(
        !names.iter().any(|n| n.contains("keep.py")),
        "deleted tracked file should not appear with AM filter, got {names:?}"
    );
}

#[test]
fn resolve_changed_skips_dir_prefix_ignore() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("x.py"), "z=1\n").unwrap();
    let rel = vec!["sub/x.py".to_string()];
    let ign = vec!["sub".to_string()];
    let out = resolve_changed_source_paths(&root, &rel, &ign, None);
    assert!(out.is_empty());
}

#[test]
fn resolve_changed_skips_missing_file() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let rel = vec!["nope.py".to_string()];
    let out = resolve_changed_source_paths(&root, &rel, &[], None);
    assert!(out.is_empty());
}

#[test]
fn subdir_cwd_still_resolves_paths() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("root.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("x.py"), "z=1\n").unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(&sub).unwrap();
    let repo_root = git_repo_root(std::env::current_dir().unwrap().as_path()).unwrap();
    let names = changed_paths_commit(&repo_root).unwrap();
    let abs = resolve_changed_source_paths(&repo_root, &names, &[], None);
    std::env::set_current_dir(orig).unwrap();
    assert!(abs.iter().any(|p| p.ends_with("x.py")));
}

#[test]
fn git_output_err_on_invalid_git_invocation() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    assert!(git_output(tmp.path(), &["not-a-real-subcommand-xyz"]).is_err());
}

#[test]
fn git_ok_false_for_invalid_ref() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    assert!(!git_ok(
        tmp.path(),
        &["rev-parse", "--verify", "--quiet", "not-a-branch-xyz"]
    ));
}

#[test]
fn resolve_main_branch_name_finds_local_main() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("f.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    let name = resolve_main_branch_name(tmp.path(), None, None).unwrap();
    assert!(!name.is_empty());
}

#[test]
fn merge_base_timestamp_and_list_refs() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("f.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    let head = merge_base(tmp.path(), "HEAD").unwrap();
    assert!(!head.is_empty());
    let _ts = commit_timestamp(tmp.path(), &head).unwrap();
    let _b = current_branch_short(tmp.path());
    let _refs = list_other_refs(tmp.path(), "main").unwrap();
}

#[test]
fn changed_paths_since_head_includes_worktree() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    std::fs::write(tmp.path().join("a.py"), "x=2\n").unwrap();
    let names = changed_paths_since(tmp.path(), "HEAD").unwrap();
    assert!(names.iter().any(|n| n.contains("a.py")));
}

#[test]
fn resolve_changed_respects_ignore_and_lang() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::write(tmp.path().join("skip_me.py"), "x=1\n").unwrap();
    let rel = vec!["skip_me.py".to_string()];
    let ign = vec!["skip_me.py".to_string()];
    let out = resolve_changed_source_paths(&root, &rel, &ign, Some(crate::test_git::TestLangFilter::Rust));
    assert!(out.is_empty());
}

#[test]
fn resolve_diff_target_commit_is_none() {
    let tmp = TempDir::new().unwrap();
    let r = resolve_diff_target(
        tmp.path(),
        crate::test_git::TestChangeMode::Commit,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(r.is_none());
}

#[test]
fn auto_detect_fork_with_two_branches() {
    let tmp = TempDir::new().unwrap();
    init_repo(&tmp);
    std::fs::write(tmp.path().join("f.py"), "x=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["checkout", "-b", "feature"])
        .status()
        .unwrap();
    std::fs::write(tmp.path().join("g.py"), "y=1\n").unwrap();
    git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["commit", "-m", "f"])
        .status()
        .unwrap();
    let sha = auto_detect_fork_commit(tmp.path()).unwrap();
    assert!(!sha.is_empty());
}
