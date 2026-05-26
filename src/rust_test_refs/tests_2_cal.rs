use super::*;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_path_is_under_tests() {
    assert!(super::path_is_under_tests(Path::new("tests/foo.rs")));
    assert!(!super::path_is_under_tests(Path::new("src/lib.rs")));
}

#[test]
fn test_seed_binary_entry_roots_finds_bin_main() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bin_dir = tmp.path().join("src/bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_rs = bin_dir.join("app.rs");
    std::fs::write(&bin_rs, "fn run() {}\n").unwrap();
    let parsed_bin = parse_rust_file(&bin_rs).unwrap();
    let mut refs = HashSet::new();
    super::seed_binary_entry_roots(&[&parsed_bin], &mut refs);
    assert!(refs.contains("run"));
}

#[test]
fn test_seed_binary_entry_roots_from_item() {
    let ast: syn::File = syn::parse_str("fn main() {}\nfn run() {}\n").unwrap();
    let mut refs = HashSet::new();
    for item in &ast.items {
        super::seed_binary_entry_roots_from_item(item, &mut refs);
    }
    assert!(refs.contains("main"));
    assert!(refs.contains("run"));
}

#[test]
fn test_expand_coverage_references_to_fixpoint_direct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(
        &prod,
        "pub fn a() { b(); }\npub fn b() { c(); }\npub fn c() {}\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&prod).unwrap();
    let mut refs = HashSet::from(["a".to_string()]);
    super::expand_coverage_references_to_fixpoint(&[&parsed], &mut refs);
    assert!(refs.contains("c"));
}

#[test]
fn test_expand_coverage_references_one_hop_direct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(&prod, "pub fn entry() { helper(); }\npub fn helper() {}\n").unwrap();
    let parsed = parse_rust_file(&prod).unwrap();
    let mut refs = HashSet::from(["entry".to_string()]);
    super::expand_coverage_references_one_hop(&[&parsed], &mut refs);
    assert!(refs.contains("helper"));
}

#[test]
fn test_expand_one_hop_from_item_direct() {
    let ast: syn::File = syn::parse_str("fn entry() { leaf(); }\nfn leaf() {}\n").unwrap();
    let refs = HashSet::from(["entry".to_string()]);
    let mut added = HashSet::new();
    for item in &ast.items {
        super::expand_one_hop_from_item(item, &refs, &mut added);
    }
    assert!(added.contains("leaf"));
}

#[test]
fn test_merge_one_hop_refs_direct() {
    let refs = HashSet::from(["seen".to_string()]);
    let mut added = HashSet::new();
    let body = HashSet::from(["seen".to_string(), "new".to_string()]);
    super::merge_one_hop_refs(body, &refs, &mut added);
    assert!(!added.contains("seen"));
    assert!(added.contains("new"));
}

#[test]
fn test_expand_coverage_one_hop_through_impl_method() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(
        &prod,
        "struct S;\nimpl S {\n    pub fn caller() { helper(); }\n    fn helper() {}\n}\n",
    )
    .unwrap();
    let test = tmp.path().join("s_test.rs");
    std::fs::write(
        &test,
        "#[test]\nfn t() { S::caller(); }\n",
    )
    .unwrap();
    let parsed_prod = parse_rust_file(&prod).unwrap();
    let parsed_test = parse_rust_file(&test).unwrap();
    let cal = analyze_rust_test_refs_for_coverage_map(&[&parsed_prod, &parsed_test], None);
    assert!(
        !cal
            .unreferenced
            .iter()
            .any(|d| d.name == "helper"),
        "one-hop through impl method body should cover helper"
    );
}

#[test]
fn test_expand_coverage_one_hop_from_test_call() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(
        &prod,
        "pub fn entry() { helper_a(); helper_b(); }\npub fn helper_a() {}\npub fn helper_b() {}\n",
    )
    .unwrap();
    let test = tmp.path().join("entry_test.rs");
    std::fs::write(&test, "#[test]\nfn t() { entry(); helper_a(); }\n").unwrap();
    let parsed_prod = parse_rust_file(&prod).unwrap();
    let parsed_test = parse_rust_file(&test).unwrap();
    let cal = analyze_rust_test_refs_for_coverage_map(&[&parsed_prod, &parsed_test], None);
    assert!(
        !cal
            .unreferenced
            .iter()
            .any(|d| d.name == "helper_b"),
        "one-hop should cover helper_b when entry is called from a test"
    );
}
