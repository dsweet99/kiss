use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{qualify_colliding_entries, rust_selector_test_path, universe_rust_selectors_for_file};

fn demo_crate() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub mod a;\npub mod b;\n").unwrap();
    for rel in ["src/a.rs", "src/b.rs", "tests/a.rs", "tests/b.rs"] {
        std::fs::write(root.join(rel), "").unwrap();
    }
    tmp
}

fn entry(root: &Path, rel: &str, selector: &str) -> (PathBuf, String) {
    (root.join(rel), selector.to_string())
}

#[test]
fn colliding_selectors_get_binary_and_module_qualified_names() {
    let tmp = demo_crate();
    let root = tmp.path();
    let entries = vec![
        entry(root, "tests/a.rs", "t_same"),
        entry(root, "tests/a.rs", "t_ok_a"),
        entry(root, "tests/b.rs", "t_same"),
        entry(root, "src/a.rs", "tests::same"),
        entry(root, "src/b.rs", "tests::same"),
    ];
    let selectors: Vec<String> = qualify_colliding_entries(root, entries)
        .into_iter()
        .map(|(_, selector)| selector)
        .collect();
    assert_eq!(
        selectors,
        [
            "demo::a$t_same",
            "t_ok_a",
            "demo::b$t_same",
            "demo::demo$a::tests::same",
            "demo::demo$b::tests::same",
        ]
    );
}

#[test]
fn unique_selectors_stay_bare() {
    let tmp = demo_crate();
    let root = tmp.path();
    let entries = vec![
        entry(root, "tests/a.rs", "t_one"),
        entry(root, "tests/b.rs", "t_two"),
        entry(root, "tests/b.rs", "t_two"),
    ];
    assert_eq!(qualify_colliding_entries(root, entries.clone()), entries);
}

#[test]
fn file_selectors_map_to_the_universe_form() {
    let tmp = demo_crate();
    let root = tmp.path();
    let universe: BTreeSet<String> = ["demo::a$t_same", "demo::b$t_same", "t_ok_a"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        universe_rust_selectors_for_file(
            root,
            &root.join("tests/b.rs"),
            vec!["t_same".into(), "t_ok_a".into(), "t_new".into()],
            &universe,
        ),
        ["demo::b$t_same", "t_ok_a", "t_new"]
    );
}

#[test]
fn test_path_strips_the_binary_qualifier() {
    assert_eq!(
        rust_selector_test_path("demo::demo$a::tests::same"),
        "a::tests::same"
    );
    assert_eq!(rust_selector_test_path("$a::tests::same"), "a::tests::same");
    assert_eq!(rust_selector_test_path("tests::same"), "tests::same");
}
