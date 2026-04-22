use super::*;
use std::io::Write;

#[test]
fn test_helpers() {
    let f: syn::File = syn::parse_str("impl Foo { fn a(&self) {} fn b(&self) {} }").unwrap();
    if let syn::Item::Impl(i) = &f.items[0] {
        assert_eq!(count_impl_methods(i), 2);
    }
    let f2: syn::File = syn::parse_str("impl MyStruct { fn a(&self) {} }").unwrap();
    if let syn::Item::Impl(i) = &f2.items[0] {
        assert_eq!(get_impl_type_name(i), Some("MyStruct".to_string()));
    }
}

#[test]
fn test_analyzer_basic() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "fn foo() {{}}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    assert!(analyze_rust_file(&parsed, &Config::default()).is_empty());
    let p = std::path::PathBuf::from("t.rs");
    let mut v = Vec::new();
    RustAnalyzer::new(&p, &Config::default(), &mut v)
        .analyze_item(&syn::parse_str::<syn::File>("fn foo() {}").unwrap().items[0]);
}

#[test]
fn test_analyzer_checks() {
    let p = std::path::PathBuf::from("t.rs");
    let cfg = Config {
        methods_per_class: 5,
        ..Default::default()
    };
    let mut v = Vec::new();
    RustAnalyzer::new(&p, &cfg, &mut v).check_methods_per_class(1, "S", 10);
    assert_eq!(v.len(), 1);
}

#[test]
fn analyze_skips_cfg_test_mod_for_per_function_rules() {
    let body = (0..15)
        .map(|_| "let _ = 1;")
        .collect::<Vec<_>>()
        .join("\n        ");
    let src = format!("#[cfg(test)]\nmod t {{\n    fn bloated() {{\n        {body}\n    }}\n}}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        statements_per_function: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        !viols.iter().any(|v| v.metric == "statements_per_function"),
        "cfg(test) mod inner functions should not be checked: {viols:?}"
    );
}

#[test]
fn analyze_nested_mod_without_cfg_still_checked() {
    let body = (0..15)
        .map(|_| "let _ = 1;")
        .collect::<Vec<_>>()
        .join("\n        ");
    let src = format!("mod t {{\n    fn bloated() {{\n        {body}\n    }}\n}}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        statements_per_function: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "statements_per_function"),
        "expected statements_per_function violation in nested mod, got {viols:?}"
    );
}

#[test]
fn test_analyzer_impl_and_fn() {
    let p = std::path::PathBuf::from("t.rs");
    let fi: syn::File = syn::parse_str("impl Foo { fn bar(&self) { let x = 1; } }").unwrap();
    if let syn::Item::Impl(i) = &fi.items[0] {
        let mut v = Vec::new();
        RustAnalyzer::new(&p, &Config::default(), &mut v).analyze_impl_block(i);
    }
    let ff: syn::File = syn::parse_str("fn foo(x: i32) { let y = x + 1; }").unwrap();
    if let syn::Item::Fn(func) = &ff.items[0] {
        let mut v = Vec::new();
        RustAnalyzer::new(&p, &Config::default(), &mut v).analyze_function(
            "foo",
            1,
            &func.sig.inputs,
            &func.block,
            count_non_doc_attrs(&func.attrs),
            "Function",
        );
    }
}
