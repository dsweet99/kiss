use super::*;

#[test]
fn rust_product_path_includes_source_modules_with_test_like_names() {
    assert!(is_rust_product_path(Path::new("src/lib.rs")));
    for rel in [
        "src/foo_test.rs",
        "src/foo_tests.rs",
        "src/test_foo.rs",
        "src/tests_foo.rs",
        "src/test_refs/coverage_weighted/branch_tests.rs",
    ] {
        assert!(
            is_rust_product_path(Path::new(rel)),
            "{rel} is compiled product code unless it lives under tests/ or target/"
        );
    }
    assert!(!is_rust_product_path(Path::new("tests/foo_test.rs")));
    assert!(!is_rust_product_path(Path::new("target/debug/build.rs")));
}

#[test]
fn normalize_against_keeps_unrelated_and_relative_paths() {
    assert_eq!(
        normalize_against(Path::new("/repo"), "/repo/src/lib.rs"),
        PathBuf::from("src/lib.rs")
    );
    assert_eq!(
        normalize_against(Path::new("/repo"), "/elsewhere/src/lib.rs"),
        PathBuf::from("/elsewhere/src/lib.rs")
    );
    assert_eq!(
        normalize_against(Path::new("/repo"), "src/lib.rs"),
        PathBuf::from("src/lib.rs")
    );
}

#[test]
fn empty_analysis_has_no_runtime_coverage_state() {
    let analysis = empty_analysis();

    assert!(analysis.definitions.is_empty());
    assert!(analysis.test_references.is_empty());
    assert!(analysis.call_references.is_empty());
    assert!(analysis.propagated_references.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}
