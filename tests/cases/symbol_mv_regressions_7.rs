//! Regression: `kiss mv` must not rewrite identifier occurrences inside
//! comments or string literals in files other than the moved-definition body.
//!
//! Existing test `regression_move_rename_should_not_touch_comments_or_strings`
//! (`symbol_mv_regressions_3.rs`) only covers the moved-definition body, which
//! is filtered through `rename_definition_text` -> `is_code_offset`. The
//! reference-edit path (`collect_reference_edits` /
//! `collect_source_rename_edits`) has no string/comment awareness, so it
//! corrupts comments and string literals in other files.
//!
//! See `_kpop/exp_log_mv_serious_bug_2.md` (H1).

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn regression_rust_rename_should_not_touch_comments_or_strings_in_other_files() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("a.rs");
    let other = tmp.path().join("b.rs");

    fs::write(&src, "pub fn helper() -> i32 { 1 }\n").unwrap();
    fs::write(
        &other,
        "\
use crate::a::helper;

/// Doc comment that mentions helper() in prose.
pub fn caller() -> i32 {
    // Inline comment: remember to call helper().
    let label = \"helper() literal\";
    let _ = label;
    helper()
}
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", src.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Rust),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated_src = fs::read_to_string(&src).unwrap();
    let updated_other = fs::read_to_string(&other).unwrap();

    assert!(
        updated_src.contains("pub fn renamed()"),
        "source definition should be renamed; got:\n{updated_src}"
    );

    assert!(
        updated_other.contains("use crate::a::renamed;"),
        "real `use` import should be updated; got:\n{updated_other}"
    );
    assert!(
        updated_other.contains("    renamed()\n"),
        "real call site should be updated; got:\n{updated_other}"
    );

    assert!(
        updated_other.contains("/// Doc comment that mentions helper() in prose."),
        "/// doc comment must NOT be modified; got:\n{updated_other}"
    );
    assert!(
        updated_other.contains("// Inline comment: remember to call helper()."),
        "// inline comment must NOT be modified; got:\n{updated_other}"
    );
    assert!(
        updated_other.contains("\"helper() literal\""),
        "string literal must NOT be modified; got:\n{updated_other}"
    );
}

#[test]
fn regression_python_rename_should_not_touch_comments_or_strings_in_other_files() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("a.py");
    let other = tmp.path().join("b.py");

    fs::write(&src, "def helper():\n    return 1\n").unwrap();
    fs::write(
        &other,
        "\
from a import helper

def caller():
    # Inline comment: please invoke helper() before midnight.
    label = \"helper() string literal\"
    _ = label
    return helper()
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", src.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };

    assert_eq!(run_mv_command(opts), 0, "mv command should succeed");

    let updated_src = fs::read_to_string(&src).unwrap();
    let updated_other = fs::read_to_string(&other).unwrap();

    assert!(
        updated_src.contains("def renamed():"),
        "source definition should be renamed; got:\n{updated_src}"
    );

    assert!(
        updated_other.contains("from a import renamed"),
        "real `from a import` should be updated; got:\n{updated_other}"
    );
    assert!(
        updated_other.contains("return renamed()"),
        "real call site should be updated; got:\n{updated_other}"
    );

    assert!(
        updated_other.contains("# Inline comment: please invoke helper() before midnight."),
        "# inline comment must NOT be modified; got:\n{updated_other}"
    );
    assert!(
        updated_other.contains("\"helper() string literal\""),
        "string literal must NOT be modified; got:\n{updated_other}"
    );
}
