use super::*;
use kiss::Language;
use std::fs;
use tempfile::tempdir;

fn init_git_repo(root: &std::path::Path) {
    let mut cmd = kiss::scrubbed_git_command(root);
    assert!(cmd.arg("init").status().unwrap().success());
}

#[test]
fn expand_directory_yields_nested_py_and_rs() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let dir = tmp.path().join("dir");
    fs::create_dir_all(dir.join("nest")).unwrap();
    fs::write(dir.join("a.py"), "x = 1\n").unwrap();
    fs::write(dir.join("nest/b.rs"), "fn f() {}\n").unwrap();

    let expanded = expand_target_operands(tmp.path(), &["dir".into()], &[], None).unwrap();
    match expanded {
        ExpandedTargetPlan::Files(files) => {
            assert!(files.iter().any(|f| f.ends_with("a.py")), "{files:?}");
            assert!(files.iter().any(|f| f.ends_with("b.rs")), "{files:?}");
        }
        ExpandedTargetPlan::All => panic!("expected files"),
    }
}

#[test]
fn expand_honors_lang_and_ignore() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let dir = tmp.path().join("pkg");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.py"), "x = 1\n").unwrap();
    fs::write(dir.join("b.rs"), "fn f() {}\n").unwrap();
    fs::create_dir_all(dir.join("skip")).unwrap();
    fs::write(dir.join("skip/c.rs"), "fn g() {}\n").unwrap();

    let rust_only =
        expand_target_operands(tmp.path(), &["pkg".into()], &[], Some(Language::Rust)).unwrap();
    match rust_only {
        ExpandedTargetPlan::Files(files) => {
            assert!(files.iter().all(|f| f.ends_with(".rs")), "{files:?}");
            assert_eq!(files.len(), 2);
        }
        ExpandedTargetPlan::All => panic!("expected files"),
    }

    let ignored = expand_target_operands(
        tmp.path(),
        &["pkg".into()],
        &["skip".into()],
        Some(Language::Rust),
    )
    .unwrap();
    match ignored {
        ExpandedTargetPlan::Files(files) => {
            assert_eq!(files.len(), 1);
            assert!(files[0].ends_with("b.rs"));
        }
        ExpandedTargetPlan::All => panic!("expected files"),
    }
}

#[test]
fn expand_lang_filter_on_other_language_dir_is_empty_not_zero_files() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let dir = tmp.path().join("nested");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src").join("lib.rs"), "pub fn n() {}\n").unwrap();
    let expanded =
        expand_target_operands(tmp.path(), &["nested".into()], &[], Some(Language::Python))
            .unwrap();
    match expanded {
        ExpandedTargetPlan::Files(files) => assert!(files.is_empty(), "{files:?}"),
        ExpandedTargetPlan::All => panic!("expected empty files, not all"),
    }
}

#[test]
fn expand_empty_and_missing_fail_fast() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    fs::create_dir_all(tmp.path().join("empty")).unwrap();
    let empty_err = expand_target_operands(tmp.path(), &["empty".into()], &[], None).unwrap_err();
    assert!(empty_err.contains("empty"), "{empty_err}");
    assert!(empty_err.contains("zero"), "{empty_err}");

    let missing_err =
        expand_target_operands(tmp.path(), &["missing".into()], &[], None).unwrap_err();
    assert!(missing_err.contains("not found"), "{missing_err}");
}

#[test]
fn expand_sole_repo_root_is_all_and_mix_errors() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let root_s = root.to_string_lossy().into_owned();

    assert!(matches!(
        expand_target_operands(tmp.path(), std::slice::from_ref(&root_s), &[], None).unwrap(),
        ExpandedTargetPlan::All
    ));

    let mix_err =
        expand_target_operands(tmp.path(), &[root_s, "lib.rs".into()], &[], None).unwrap_err();
    assert!(mix_err.contains("mixed"), "{mix_err}");
}
