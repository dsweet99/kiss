use crate::rust_test_refs::*;
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
    let tokio_test: syn::File = syn::parse_str("#[tokio::test]\nasync fn t() {}").unwrap();
    if let syn::Item::Fn(f) = &tokio_test.items[0] {
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
        end_line: 1,
        impl_for_type: Some("MyType".into()),
    };
    let refs: HashSet<String> = ["MyType", "foo"].into_iter().map(String::from).collect();
    assert!(is_impl_with_referenced_type(&def, &refs));
    let def2 = RustCodeDefinition {
        name: "foo".into(),
        kind: CodeUnitKind::Function,
        file: "t.rs".into(),
        line: 1,
        end_line: 1,
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
    let ty: syn::Type = syn::parse_str("MyType").unwrap();
    references::ReferenceVisitor {
        refs: &mut refs,
        mode: references::RefWitnessMode::GATE,
    }
    .visit_type(&ty);
    assert!(refs.contains("MyType"));
    let mac: syn::ExprMacro = syn::parse_str("println!(\"test\")").unwrap();
    references::ReferenceVisitor {
        refs: &mut refs,
        mode: references::RefWitnessMode::GATE,
    }
    .visit_macro(&mac.mac);
    let tokens1: proc_macro2::TokenStream = "foo()".parse().unwrap();
    assert!(references::try_parse_as_single_expr(
        &tokens1,
        &mut refs,
        references::RefWitnessMode::GATE
    ));
    let tokens2: proc_macro2::TokenStream = "a, b".parse().unwrap();
    assert!(references::try_parse_as_expr_list(
        &tokens2,
        &mut refs,
        references::RefWitnessMode::GATE
    ));
    let tokens3: proc_macro2::TokenStream = "{ bar() }".parse().unwrap();
    references::visit_nested_token_groups(&tokens3, &mut refs, references::RefWitnessMode::GATE);
}

#[test]
fn test_rule_variant_snake_alias_in_coverage_map_macro_args() {
    let mut refs = HashSet::new();
    let tokens: proc_macro2::TokenStream =
        "Rule::ShebangNotExecutable, Path::new(\"x.py\")".parse().unwrap();
    assert!(references::try_parse_as_expr_list(
        &tokens,
        &mut refs,
        references::RefWitnessMode::COVERAGE_MAP
    ));
    assert!(refs.contains("ShebangNotExecutable"));
    assert!(refs.contains("shebang_not_executable"));
}

#[test]
fn test_test_case_semicolon_separated_macro_tokens() {
    let mut refs = HashSet::new();
    let tokens: proc_macro2::TokenStream = "Rule::LineContainsTodo; \"T003\"".parse().unwrap();
    references::visit_macro_tokens(
        &tokens,
        &mut refs,
        references::RefWitnessMode::COVERAGE_MAP,
    );
    assert!(refs.contains("LineContainsTodo"));
    assert!(refs.contains("line_contains_todo"));
}

#[test]
fn test_collect_test_parametric_attr_skips_non_list_meta() {
    let mut refs = HashSet::new();
    let attrs = vec![syn::parse_quote!(#[test_case = "not-a-list"])];
    references::collect_test_parametric_attr_macro_witnesses(
        &attrs,
        &mut refs,
        references::RefWitnessMode::COVERAGE_MAP,
    );
    assert!(refs.is_empty());
}

#[test]
fn test_rstest_parametric_attribute_helpers() {
    let attr: syn::Attribute = syn::parse_quote!(#[rstest::rstest]);
    assert!(references::is_test_parametric_attribute(&attr));
    let attrs = vec![
        syn::parse_quote!(#[case]),
        syn::parse_quote!(#[should_panic]),
    ];
    assert!(references::has_test_parametric_attribute(&attrs));
    assert!(!references::is_test_parametric_attribute(&attrs[1]));
}

#[test]
fn test_collect_fn_test_attr_macro_witnesses_nested_mod() {
    let code = r#"
mod outer {
    #[test_case(Rule::Foo)]
    fn inner() {}
}
#[test_case(Rule::Bar, Rule::Baz)]
fn top() {}
"#;
    let ast: syn::File = syn::parse_str(code).unwrap();
    let mut refs = HashSet::new();
    references::collect_fn_test_attr_macro_witnesses(
        &ast.items,
        &mut refs,
        references::RefWitnessMode::COVERAGE_MAP,
    );
    assert!(refs.contains("Foo"));
    assert!(refs.contains("Bar"));
    assert!(refs.contains("Baz"));
}

