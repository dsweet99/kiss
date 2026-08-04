use super::*;
use kiss::Language;
use std::fs;
use tempfile::tempdir;

fn init_git_repo(root: &std::path::Path) {
    let mut cmd = kiss::scrubbed_git_command(root);
    assert!(cmd.arg("init").status().unwrap().success());
}

#[test]
fn resolve_rejects_lang_and_ignore_mismatches() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let py = tmp.path().join("app.py");
    fs::write(&py, "def value():\n    return 1\n").unwrap();

    let err = resolve_target_operands(
        tmp.path(),
        &["app.py".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap_err();
    assert!(err.contains("--lang"));

    let err = resolve_target_operands(
        tmp.path(),
        &["app.py".into()],
        None,
        &["app.py".into()],
        &[],
    )
    .unwrap_err();
    assert!(err.contains("--ignore"));
}

#[test]
fn resolve_path_symbol_uses_definition_lines() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let path = tmp.path().join("lib.rs");
    fs::write(&path, "pub fn alpha() {}\npub fn beta() {}\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["lib.rs::alpha".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert!(query.direct_rust.is_empty());
    assert!(query.rust_files.is_empty());
    assert_eq!(query.rust_lines.len(), 1);
    let lines = query.rust_lines.values().next().unwrap();
    assert!(lines.contains(&1));
    assert!(!lines.contains(&2));
}

#[test]
fn resolve_path_uses_file_level_not_line_map() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let path = tmp.path().join("lib.rs");
    fs::write(&path, "pub fn alpha() {}\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["lib.rs".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(query.rust_files.len(), 1);
    assert!(query.rust_lines.is_empty());
    let model = super::model::load_source_model(&path, Language::Rust).unwrap();
    assert!(!model.non_test_lines().is_empty());
    assert_eq!(model.all_lines().len() as u32, model.line_count);
    assert!(model.direct_test_lines().is_empty());
}

#[test]
fn resolve_const_symbol_uses_definition_span() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let path = tmp.path().join("lib.rs");
    fs::write(&path, "pub(super) const RS_RULE_SPECS: &[u8] = &[1, 2];\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["lib.rs::RS_RULE_SPECS".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert!(query.rust_files.is_empty());
    assert_eq!(query.rust_lines.len(), 1);
    assert!(query.rust_lines.values().next().unwrap().contains(&1));
}

#[test]
fn resolve_missing_and_unresolved_targets_error() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let path = tmp.path().join("lib.rs");
    fs::write(&path, "pub fn alpha() {}\n").unwrap();

    assert!(
        resolve_target_operands(tmp.path(), &["missing.rs".into()], None, &[], &[]).is_err()
    );
    assert!(
        resolve_target_operands(tmp.path(), &["lib.rs::missing".into()], None, &[], &[])
            .is_err()
    );
}

#[test]
fn resolve_deduplicates_repeated_operands() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let path = tmp.path().join("lib.rs");
    fs::write(&path, "pub fn alpha() {}\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["lib.rs".into(), "lib.rs".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(query.rust_files.len(), 1);
}
