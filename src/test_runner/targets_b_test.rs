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

    assert!(resolve_target_operands(tmp.path(), &["missing.rs".into()], None, &[], &[]).is_err());
    assert!(
        resolve_target_operands(tmp.path(), &["lib.rs::missing".into()], None, &[], &[]).is_err()
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

#[test]
fn resolve_python_parametrized_nodeid_is_direct_selector() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();

    fs::write(
        tests.join("conftest.py"),
        "def pytest_configure(config):\n    raise RuntimeError('collect must not run')\n",
    )
    .unwrap();
    fs::write(
        tests.join("test_params.py"),
        "import pytest\n\n@pytest.mark.parametrize('name', ['a.py', 'b.py'])\ndef test_item(name):\n    assert name.endswith('.py')\n",
    )
    .unwrap();
    let nodeid = "tests/test_params.py::test_item[a.py]";
    let query = resolve_target_operands(
        tmp.path(),
        &[nodeid.into()],
        Some(Language::Python),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(
        query.direct_python.iter().collect::<Vec<_>>(),
        vec![&nodeid.to_string()]
    );
}

#[test]
fn resolve_python_test_file_path_is_direct_only() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(tests.join("test_a.py"), "def test_x():\n    assert True\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["tests/test_a.py".into()],
        Some(Language::Python),
        &[],
        &[],
    )
    .unwrap();
    assert!(
        query.direct_python.iter().any(|s| s.contains("test_x")),
        "expected collected test selector, got {:?}",
        query.direct_python
    );
    assert!(
        query.python_files.is_empty(),
        "test-file path must not be a coverage source"
    );
    assert!(query.python_lines.is_empty());
}

#[test]
fn resolve_rust_test_file_path_is_direct_only() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "").unwrap();
    let tests = tmp.path().join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(tests.join("smoke.rs"), "#[test]\nfn case_one() {}\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["tests/smoke.rs".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert!(!query.direct_rust.is_empty());
    assert!(
        query.rust_files.is_empty(),
        "rust test-file path must not be a coverage source"
    );
}

#[test]
fn resolve_mixed_file_test_only_helper_is_not_coverage_target() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        "pub fn prod() {}\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n    #[test]\n    fn t() {}\n}\n",
    )
    .unwrap();

    let helper = resolve_target_operands(
        tmp.path(),
        &["src/lib.rs::helper".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert!(
        helper.rust_lines.is_empty() && helper.rust_files.is_empty(),
        "test-only helper must not become a production coverage target, got {:?}",
        helper.rust_lines
    );

    let prod = resolve_target_operands(
        tmp.path(),
        &["src/lib.rs::prod".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert!(
        !prod.rust_lines.is_empty(),
        "production symbol must remain a coverage target"
    );
}

#[test]
fn resolve_ignored_rust_test_remains_explicit_selector() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "#[test]\n#[ignore]\nfn skipped() {}\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["src/lib.rs::skipped".into()],
        Some(Language::Rust),
        &[],
        &[],
    )
    .unwrap();
    assert!(
        query.direct_rust.iter().any(|s| s.contains("skipped")),
        "ignored #[test] must remain an explicit selector, got {:?}",
        query.direct_rust
    );
    assert!(query.rust_files.is_empty());
    assert!(query.rust_lines.is_empty());
}

#[test]
fn resolve_non_test_source_path_still_inserts_file() {
    let tmp = tempdir().unwrap();
    init_git_repo(tmp.path());
    let path = tmp.path().join("app.py");
    fs::write(&path, "def value():\n    return 1\n").unwrap();
    let query = resolve_target_operands(
        tmp.path(),
        &["app.py".into()],
        Some(Language::Python),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(query.python_files.len(), 1);
    assert!(query.direct_python.is_empty());
}
