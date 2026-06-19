use super::*;
use crate::rust_parsing::parse_rust_file;
use std::io::Write;
use syn::Item;

#[test]
fn test_file_detection_and_helpers() {
    assert!(!is_rust_test_file(Path::new("src/cache_tests.rs")));
    assert!(!is_rust_test_file(Path::new("test_utils.rs")));
    assert!(!is_rust_test_file(Path::new("utils_test.rs")));
    assert!(!is_rust_test_file(Path::new("src/main.rs")));
    assert!(is_rs_file(Path::new("foo.rs")) && !is_rs_file(Path::new("foo.py")));
    assert!(
        is_rs_file(Path::new("foo.RS")),
        ".RS extension must match Rust (Path::extension preserves case)"
    );
    assert!(
        is_rust_test_file(Path::new("tests/helpers/too_many_args.rs")),
        "Rust files under test directories are test files; repo fixtures are excluded by discovery boundaries"
    );
    assert!(definitions::is_private("_helper") && !definitions::is_private("helper"));
    assert!(references::is_rust_keyword("self") && !references::is_rust_keyword("foo"));
    let ty: syn::Type = syn::parse_str("Foo").unwrap();
    assert_eq!(definitions::extract_type_name(&ty), Some("Foo".into()));
    let _ = RustTestRefAnalysis {
        definitions: vec![],
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        propagated_references: HashSet::new(),
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
    assert!(
        has_inline_test_module(&f2),
        "source-style inline test modules should be recognized from the AST"
    );
    let f: syn::File = syn::parse_str("fn foo() {}\nstruct Bar {}").unwrap();
    let mut defs = Vec::new();
    definitions::collect_rust_definitions(&f, Path::new("t.rs"), &mut defs);
    let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(
        !names.contains(&"Bar"),
        "bare type declarations are not executable coverage targets"
    );
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
    assert!(!is_impl_method_covered_by_type_and_name(&def, &refs));
    assert!(is_directly_referenced(
        &def2,
        &refs,
        &name_files,
        &disambiguation
    ));
    assert!(!is_covered_by_tests(
        &def,
        &refs,
        &HashSet::new(),
        &name_files,
        &disambiguation
    ));
    let mut refs_with_fmt = refs.clone();
    refs_with_fmt.insert("fmt".to_string());
    assert!(is_covered_by_tests(
        &def,
        &refs_with_fmt,
        &HashSet::new(),
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
    let mut qualified = HashSet::new();
    let _ = references::ReferenceVisitor {
        refs: &mut refs,
        qualified: &mut qualified,
    };
    let ty: syn::Type = syn::parse_str("MyType").unwrap();
    references::ReferenceVisitor {
        refs: &mut refs,
        qualified: &mut qualified,
    }
    .visit_type(&ty);
    assert!(
        !refs.contains("MyType"),
        "type-position refs must not count as coverage witnesses"
    );
    let mac: syn::ExprMacro = syn::parse_str("println!(\"test\")").unwrap();
    references::ReferenceVisitor {
        refs: &mut refs,
        qualified: &mut qualified,
    }
    .visit_macro(&mac.mac);
    let tokens1: proc_macro2::TokenStream = "foo()".parse().unwrap();
    assert!(references::try_parse_as_single_expr(
        &tokens1,
        &mut refs,
        &mut qualified
    ));
    let tokens2: proc_macro2::TokenStream = "a, b".parse().unwrap();
    assert!(references::try_parse_as_expr_list(
        &tokens2,
        &mut refs,
        &mut qualified
    ));
    let tokens3: proc_macro2::TokenStream = "{ bar() }".parse().unwrap();
    references::visit_nested_token_groups(&tokens3, &mut refs, &mut qualified);
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
fn external_cfg_test_module_files_are_not_product_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let parent = tmp.path().join("lib.rs");
    let helper = tmp.path().join("helper_tests.rs");
    std::fs::write(
        &parent,
        "#[cfg(test)] mod helper_tests;\npub fn product() {}\n",
    )
    .unwrap();
    std::fs::write(&helper, "pub fn fixture_helper() {}\n").unwrap();
    let parsed_parent = parse_rust_file(&parent).unwrap();
    let parsed_helper = parse_rust_file(&helper).unwrap();

    let analysis = analyze_rust_test_refs(&[&parsed_parent, &parsed_helper], None);

    let names: Vec<_> = analysis
        .definitions
        .iter()
        .map(|def| def.name.as_str())
        .collect();
    assert!(names.contains(&"product"));
    assert!(!names.contains(&"fixture_helper"));
}

#[test]
fn test_like_product_module_files_remain_product_definitions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let parent = tmp.path().join("lib.rs");
    let module = tmp.path().join("cache_tests.rs");
    std::fs::write(&parent, "mod cache_tests;\n").unwrap();
    std::fs::write(&module, "pub fn cache_product() {}\n").unwrap();
    let parsed_parent = parse_rust_file(&parent).unwrap();
    let parsed_module = parse_rust_file(&module).unwrap();

    let analysis = analyze_rust_test_refs(&[&parsed_parent, &parsed_module], None);

    assert!(
        analysis
            .definitions
            .iter()
            .any(|def| def.name == "cache_product")
    );
}

#[test]
fn test_collect_rust_references() {
    let ast: syn::File = syn::parse_str("fn test() { foo(); bar::baz(); }").unwrap();
    let mut refs = HashSet::new();
    let mut qualified = HashSet::new();
    references::collect_rust_references(&ast, &mut refs, &mut qualified);
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
    std::fs::write(&test_path, "#[test]\nfn t() { alpha::helper(); }").unwrap();

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
