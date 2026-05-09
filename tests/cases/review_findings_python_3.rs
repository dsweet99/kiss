//! Additional Python regressions for parser-first `kiss mv`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

#[test]
fn review_python_move_should_not_rename_shadowed_inner_helper() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("a.py");
    let dest = tmp.path().join("dest.py");
    fs::write(
        &source,
        "\
def helper():
    return 1


def outer():
    def helper():
        return 2
    return helper()
",
    )
    .unwrap();

    let opts = MvOptions {
        query: format!("{}::helper", source.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: Some(dest.clone()),
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_source = fs::read_to_string(&source).unwrap();
    let updated_dest = fs::read_to_string(&dest).unwrap();
    assert!(
        updated_dest.contains("def renamed():"),
        "moved helper should be renamed in destination; got:\n{updated_dest}"
    );
    assert!(
        updated_source.contains("    def helper():\n        return 2"),
        "shadowed inner helper must remain unchanged in source; got:\n{updated_source}"
    );
    assert!(
        updated_source.contains("    return helper()"),
        "shadowed inner call must remain unchanged in source; got:\n{updated_source}"
    );
}
