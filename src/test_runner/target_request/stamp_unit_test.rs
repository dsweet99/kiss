use super::capture_worktree_token;
use crate::test_runner::test_mode_fixtures::{git_in, init_git};
use std::fs;

fn bilingual_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join(".gitignore"), "/target\n/.kiss\n").unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "bilingual"])
            .status()
            .unwrap()
            .success()
    );
    tmp
}

#[test]

fn lang_filtered_worktree_ignores_other_language() {
    // kt_bug.md class #15 lock (semantic B): `--lang rust` worktree must ignore
    // Python-only edits; `--lang python` must ignore Rust-only edits.
    let tmp = bilingual_repo();
    let root = tmp.path();

    let rust_before = capture_worktree_token(root, Some(kiss::Language::Rust));
    fs::write(root.join("app.py"), "x = 2\n").unwrap();
    let rust_after_py = capture_worktree_token(root, Some(kiss::Language::Rust));
    assert_eq!(
        rust_before, rust_after_py,
        "--lang rust worktree must not move on Python-only edit"
    );
    fs::write(root.join("src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
    let rust_after_rs = capture_worktree_token(root, Some(kiss::Language::Rust));
    assert_ne!(
        rust_after_py, rust_after_rs,
        "--lang rust worktree must move on Rust source edit"
    );

    // Reset Rust, probe Python partition.
    fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    let py_before = capture_worktree_token(root, Some(kiss::Language::Python));
    fs::write(root.join("src/lib.rs"), "pub fn a() { /* again */ }\n").unwrap();
    let py_after_rs = capture_worktree_token(root, Some(kiss::Language::Python));
    assert_eq!(
        py_before, py_after_rs,
        "--lang python worktree must not move on Rust-only edit"
    );
    fs::write(root.join("app.py"), "x = 3\n").unwrap();
    let py_after_py = capture_worktree_token(root, Some(kiss::Language::Python));
    assert_ne!(
        py_after_rs, py_after_py,
        "--lang python worktree must move on Python source edit"
    );

    // None stays bilingual: other-lang edits still move the token.
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    let none_before = capture_worktree_token(root, None);
    fs::write(root.join("app.py"), "x = 9\n").unwrap();
    let none_after_py = capture_worktree_token(root, None);
    assert_ne!(
        none_before, none_after_py,
        "unfiltered worktree must still move on Python edit"
    );
}
