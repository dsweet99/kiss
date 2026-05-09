use super::*;
use crate::rust_parsing::parse_rust_file;
use std::io::Write;
use syn::Item;

#[test]
fn test_file_detection_and_helpers() {
    assert!(
        is_rust_test_file(Path::new("test_utils.rs"))
            && is_rust_test_file(Path::new("utils_test.rs"))
    );
    assert!(!is_rust_test_file(Path::new("src/main.rs")));
    assert!(is_rs_file(Path::new("foo.rs")) && !is_rs_file(Path::new("foo.py")));
    assert!(
        is_rs_file(Path::new("foo.RS")),
        ".RS extension must match Rust (Path::extension preserves case)"
    );
    assert!(
        is_rust_test_file(Path::new("bar_test.RS")),
        "Rust test file detection must accept uppercase .RS"
    );
    assert!(
        has_test_naming_pattern(Path::new("test_foo.rs"))
            && !has_test_naming_pattern(Path::new("foo.rs"))
    );
    assert!(definitions::is_private("_helper") && !definitions::is_private("helper"));
    assert!(references::is_rust_keyword("self") && !references::is_rust_keyword("foo"));
    let ty: syn::Type = syn::parse_str("Foo").unwrap();
    assert_eq!(definitions::extract_type_name(&ty), Some("Foo".into()));
    let _ = RustTestRefAnalysis {
        definitions: vec![],
        test_references: HashSet::new(),
        unreferenced: vec![],
        coverage_map: HashMap::new(),
    };
}

#[test]
fn test_definitions_and_references() {
    let f1: syn::File = syn::parse_str("#[test]\nfn t() {}").unwrap();
    let f2: syn::File = syn::parse_str("#[cfg(test)]\nmod tests {}").unwrap();
    if let syn::Item::Fn(f) = &f1.items[0] {
        assert!(has_test_attribute(&f.attrs));
    }
    if let syn::Item::Mod(m) = &f2.items[0] {
        assert!(has_cfg_test_attribute(&m.attrs));
    }
    let f: syn::File = syn::parse_str("fn foo() {}\nstruct Bar {}").unwrap();
    let mut defs = Vec::new();
    definitions::collect_rust_definitions(&f, Path::new("t.rs"), &mut defs);
    assert!(defs.len() >= 2);
    for item in &f.items {
        definitions::collect_definitions_from_item(item, Path::new("t.rs"), &mut defs);
    }
    let fi: syn::File = syn::parse_str("impl Foo { fn bar(&self) {} }").unwrap();
    if let Item::Impl(i) = &fi.items[0] {
        definitions::collect_impl_methods(i, Path::new("t.rs"), &mut defs);
    }
    let f3: syn::File =
        syn::parse_str("#[cfg(test)] mod tests { fn call_foo() { foo(); } }").unwrap();
    let mut refs = HashSet::new();
    definitions::collect_test_module_references(&f3, &mut refs);
    assert!(refs.contains("foo"));
}

#[test]
fn test_coverage_checks() {
    let def = RustCodeDefinition {
        name: "fmt".into(),
        kind: CodeUnitKind::TraitImplMethod,
        file: "t.rs".into(),
        line: 1,
        impl_for_type: Some("MyType".into()),
    };
    let refs: HashSet<String> = ["MyType", "foo"].into_iter().map(String::from).collect();
    assert!(is_impl_with_referenced_type(&def, &refs));
    let def2 = RustCodeDefinition {
        name: "foo".into(),
        kind: CodeUnitKind::Function,
        file: "t.rs".into(),
        line: 1,
        impl_for_type: None,
    };
    let all_definitions = [def.clone(), def2.clone()];
    let name_files = crate::test_refs::build_name_file_map(
        all_definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation = crate::test_refs::build_disambiguation_map(&name_files, &refs, &[], None);
    assert!(is_directly_referenced(
        &def2,
        &refs,
        &name_files,
        &disambiguation
    ));
    assert!(is_covered_by_tests(
        &def,
        &refs,
        &name_files,
        &disambiguation
    ));
    assert!(references::is_external_crate("std") && !references::is_external_crate("my_module"));
    let p: syn::Path = syn::parse_str("std::io").unwrap();
    assert!(references::starts_with_external_crate(&p));
}

#[test]
fn test_visitor_and_macros() {
    use syn::visit::Visit;

    let mut refs = HashSet::new();
    let _ = references::ReferenceVisitor { refs: &mut refs };
    let ty: syn::Type = syn::parse_str("MyType").unwrap();
    references::ReferenceVisitor { refs: &mut refs }.visit_type(&ty);
    assert!(refs.contains("MyType"));
    let mac: syn::ExprMacro = syn::parse_str("println!(\"test\")").unwrap();
    references::ReferenceVisitor { refs: &mut refs }.visit_macro(&mac.mac);
    let tokens1: proc_macro2::TokenStream = "foo()".parse().unwrap();
    assert!(references::try_parse_as_single_expr(&tokens1, &mut refs));
    let tokens2: proc_macro2::TokenStream = "a, b".parse().unwrap();
    assert!(references::try_parse_as_expr_list(&tokens2, &mut refs));
    let tokens3: proc_macro2::TokenStream = "{ bar() }".parse().unwrap();
    references::visit_nested_token_groups(&tokens3, &mut refs);
}

#[test]
fn test_analyze_refs() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(
        tmp,
        "fn foo() {{}}\n#[cfg(test)] mod tests {{ use super::*; #[test] fn t() {{ foo(); }} }}"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let analysis = analyze_rust_test_refs(&[&parsed], None);
    assert!(!analysis.definitions.is_empty());
    let key = (parsed.path, "foo".to_string());
    assert!(
        analysis.coverage_map.contains_key(&key),
        "coverage_map should contain foo from #[cfg(test)] mod"
    );
    let covering = &analysis.coverage_map[&key];
    assert!(
        covering.iter().any(|(_, f)| f == "tests::t"),
        "foo should be covered by tests::t, got {covering:?}"
    );
}

#[test]
fn test_collect_rust_references() {
    let ast: syn::File = syn::parse_str("fn test() { foo(); bar::baz(); }").unwrap();
    let mut refs = HashSet::new();
    references::collect_rust_references(&ast, &mut refs);
    assert!(refs.contains("foo"));
}

// === Bug-hunting tests ===

#[test]
fn test_is_external_crate_common_deps() {
    // Common Rust ecosystem crates should be recognized as external.
    // Using full external crate list from references.rs
    assert!(
        references::is_external_crate("std"),
        "std should be external"
    );
    assert!(
        references::is_external_crate("core"),
        "core should be external"
    );
}

#[test]
fn test_same_name_different_files_disambiguated_by_module() {
    let tmp = tempfile::TempDir::new().unwrap();

    let alpha_path = tmp.path().join("alpha.rs");
    std::fs::write(&alpha_path, "pub fn helper() {}").unwrap();

    let beta_path = tmp.path().join("beta.rs");
    std::fs::write(&beta_path, "pub fn helper() {}").unwrap();

    let test_path = tmp.path().join("test_alpha.rs");
    std::fs::write(&test_path, "fn t() { alpha::helper(); }").unwrap();

    let parsed_alpha = parse_rust_file(&alpha_path).unwrap();
    let parsed_beta = parse_rust_file(&beta_path).unwrap();
    let parsed_test = parse_rust_file(&test_path).unwrap();

    let analysis = analyze_rust_test_refs(&[&parsed_alpha, &parsed_beta, &parsed_test], None);

    assert_eq!(analysis.definitions.len(), 2, "both files define helper()");

    let alpha_uncovered = analysis.unreferenced.iter().any(|d| d.file == alpha_path);
    assert!(
        !alpha_uncovered,
        "alpha::helper should be covered (test imports from alpha)"
    );

    let beta_uncovered = analysis.unreferenced.iter().any(|d| d.file == beta_path);
    assert!(
        beta_uncovered,
        "beta::helper should be uncovered (no test references beta)"
    );
}
