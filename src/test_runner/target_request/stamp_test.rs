use std::fs;
use std::path::Path;

use crate::test_runner::test_mode_fixtures::{checkout_branch, git_in, git_stdout, init_git};

use super::stamp::capture_git_dep_stamp;
use super::types::GitFocus;

fn seed_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "seed"])
            .status()
            .unwrap()
            .success()
    );
    checkout_branch(tmp.path(), "feature");
    tmp
}

fn empty_commit(root: &Path) {
    assert!(
        git_in(root)
            .args(["commit", "--allow-empty", "-m", "empty"])
            .status()
            .unwrap()
            .success()
    );
}

fn stamp(root: &Path, focus: &GitFocus) -> super::stamp::GitDepStamp {
    capture_git_dep_stamp(root, focus).unwrap()
}

#[test]
fn empty_commit_is_commit_miss_and_main_hit() {
    let tmp = seed_repo();
    let root = tmp.path();
    let before_commit = stamp(root, &GitFocus::Commit);
    let before_main = stamp(root, &GitFocus::DefaultMain);
    empty_commit(root);
    assert_ne!(before_commit, stamp(root, &GitFocus::Commit));
    assert_eq!(before_main, stamp(root, &GitFocus::DefaultMain));
}

#[test]
fn empty_commit_is_base_hit_when_merge_base_holds() {
    let tmp = seed_repo();
    let root = tmp.path();
    let before = stamp(root, &GitFocus::AutomaticBase);
    empty_commit(root);
    assert_eq!(before, stamp(root, &GitFocus::AutomaticBase));
}

#[test]
fn untracked_is_commit_miss_and_base_main_hit() {
    let tmp = seed_repo();
    let root = tmp.path();
    let commit = stamp(root, &GitFocus::Commit);
    let base = stamp(root, &GitFocus::AutomaticBase);
    let main = stamp(root, &GitFocus::DefaultMain);
    fs::write(root.join("notes.md"), "tmp\n").unwrap();
    assert_ne!(commit, stamp(root, &GitFocus::Commit));
    assert_eq!(base, stamp(root, &GitFocus::AutomaticBase));
    assert_eq!(main, stamp(root, &GitFocus::DefaultMain));
}

#[test]
fn preferred_main_fallback_creation_is_a_miss() {
    let tmp = seed_repo();
    let root = tmp.path();
    let before = stamp(root, &GitFocus::DefaultMain);
    let sha = git_stdout(root, &["rev-parse", "main"]);
    assert!(
        git_in(root)
            .args(["update-ref", "refs/remotes/origin/main", &sha])
            .status()
            .unwrap()
            .success()
    );
    assert_ne!(before, stamp(root, &GitFocus::DefaultMain));
}

#[test]
fn outside_inventory_ref_move_is_automatic_base_hit() {
    let tmp = seed_repo();
    let root = tmp.path();
    let before = stamp(root, &GitFocus::AutomaticBase);
    assert!(
        git_in(root)
            .args([
                "update-ref",
                "refs/remotes/origin/feature",
                &git_stdout(root, &["rev-parse", "HEAD"])
            ])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(before, stamp(root, &GitFocus::AutomaticBase));
}

#[test]
fn new_automatic_base_candidate_is_a_miss() {
    let tmp = seed_repo();
    let root = tmp.path();
    let before = stamp(root, &GitFocus::AutomaticBase);
    assert!(
        git_in(root)
            .args(["branch", "other"])
            .status()
            .unwrap()
            .success()
    );
    assert_ne!(before, stamp(root, &GitFocus::AutomaticBase));
}

#[test]
fn commit_stamps_head_and_untracked_only() {
    let tmp = seed_repo();
    let commit = stamp(tmp.path(), &GitFocus::Commit);
    let main = stamp(tmp.path(), &GitFocus::DefaultMain);
    assert!(commit.head_oid.is_some());
    assert!(commit.untracked.is_some());
    assert!(main.head_oid.is_none());
    assert!(main.untracked.is_none());
}

#[test]
fn explicit_refs_are_not_configured_or_default() {
    let tmp = seed_repo();
    let root = tmp.path();
    let explicit = stamp(
        root,
        &GitFocus::ExplicitMain {
            branch: "main".into(),
        },
    );
    let configured = stamp(
        root,
        &GitFocus::ConfiguredMain {
            name: "main".into(),
        },
    );
    let default = stamp(root, &GitFocus::DefaultMain);
    assert_ne!(explicit, configured);
    assert_ne!(explicit, default);
    assert_ne!(configured, default);
}
