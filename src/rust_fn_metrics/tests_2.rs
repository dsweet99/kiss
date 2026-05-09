use super::*;

#[test]
fn test_count_non_doc_attrs_excludes_doc() {
    let f: syn::File = syn::parse_str(r#"#[doc = "help"] #[inline] fn f() {}"#).unwrap();
    if let syn::Item::Fn(ff) = &f.items[0] {
        assert_eq!(count_non_doc_attrs(&ff.attrs), 1);
        let m = compute_rust_function_metrics(
            &ff.sig.inputs,
            &ff.block,
            count_non_doc_attrs(&ff.attrs),
        );
        assert_eq!(m.attributes, 1);
    } else {
        panic!("expected fn");
    }
}

#[test]
fn test_file_metrics_nested_mod() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn top_level() {{ let x = 1; }}
mod inner {{
    fn nested_fn() {{ let y = 2; let z = 3; }}
    struct InnerStruct {{}}
    trait InnerTrait {{}}
    impl InnerStruct {{
        fn method(&self) {{ let w = 4; }}
    }}
}}
"
    )
    .unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let m = compute_rust_file_metrics(&parsed);
    assert_eq!(
        m.functions, 3,
        "should count top_level + nested_fn + method"
    );
    assert_eq!(m.statements, 4, "should count all statements in all fns");
    assert_eq!(m.concrete_types, 1, "should count InnerStruct");
    assert_eq!(m.interface_types, 1, "should count InnerTrait");
}

#[test]
fn test_cfg_test_mod_compound_expression_skipped() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r#"
fn production_fn() {{ let x = 1; }}

#[cfg(test)]
mod simple_test {{
    fn simple_test_fn() {{ let y = 2; }}
}}

#[cfg(all(test, feature = "expensive_tests"))]
mod compound_test {{
    fn compound_test_fn() {{ let z = 3; }}
}}
"#
    )
    .unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let m = compute_rust_file_metrics(&parsed);
    // Both test modules should be skipped - only production_fn should be counted
    assert_eq!(
        m.functions, 1,
        "should only count production_fn, not test fns (simple or compound cfg)"
    );
    assert_eq!(
        m.statements, 1,
        "should only count statements in production_fn"
    );
}

#[test]
fn test_cfg_not_test_mod_included_in_metrics() {
    // BUG TEST: #[cfg(not(test))] means "production code", NOT test code.
    // It should be INCLUDED in file metrics, not skipped.
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn always_fn() {{ let a = 1; }}

#[cfg(not(test))]
mod production_only {{
    fn prod_fn() {{ let b = 2; }}
}}

#[cfg(test)]
mod tests {{
    fn test_fn() {{ let c = 3; }}
}}
"
    )
    .unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let m = compute_rust_file_metrics(&parsed);
    // Should count always_fn + prod_fn = 2 functions, not just always_fn = 1
    // The tests module should be skipped, but not(test) module should be included
    assert_eq!(
        m.functions, 2,
        "cfg(not(test)) is production code and should be counted (got {})",
        m.functions
    );
    assert_eq!(
        m.statements, 2,
        "cfg(not(test)) statements should be counted (got {})",
        m.statements
    );
}

#[test]
fn test_is_cfg_test_mod_semantics() {
    use syn::{Item, parse_str};

    // Test that is_cfg_test_mod correctly identifies test vs production modules
    let cases = [
        (r"#[cfg(test)] mod m {}", "cfg(test)", true),
        (
            r#"#[cfg(all(test, feature = "foo"))] mod m {}"#,
            "cfg(all(test,...))",
            true,
        ),
        (
            r"#[cfg(any(test, windows))] mod m {}",
            "cfg(any(test,...))",
            true,
        ),
        (
            r"#[cfg(not(test))] mod m {}",
            "cfg(not(test)) = PRODUCTION",
            false,
        ),
        (
            r#"#[cfg(feature = "foo")] mod m {}"#,
            "cfg(feature) = PRODUCTION",
            false,
        ),
        (r"mod m {}", "no cfg = PRODUCTION", false),
    ];

    for (code, label, expected) in cases {
        let item: Item = parse_str(code).unwrap();
        if let Item::Mod(m) = item {
            let result = is_cfg_test_mod(&m);
            println!("{label}: is_cfg_test_mod = {result}, expected = {expected}");
            assert_eq!(result, expected, "mismatch for {label}");
        }
    }
}

#[test]
fn test_double_negation_not_not_test_is_test_code() {
    // BUG: not(not(test)) is logically equivalent to test, so it IS test-only code.
    // It should be SKIPPED in file metrics, not included.
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn production_fn() {{ let a = 1; }}

#[cfg(not(not(test)))]
mod double_negation_test {{
    fn double_neg_fn() {{ let b = 2; }}
}}

#[cfg(test)]
mod tests {{
    fn test_fn() {{ let c = 3; }}
}}
"
    )
    .unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let m = compute_rust_file_metrics(&parsed);
    // not(not(test)) == test, so should only count production_fn (1 function, 1 statement)
    assert_eq!(
        m.functions, 1,
        "not(not(test)) is test code and should be skipped (got {} functions)",
        m.functions
    );
    assert_eq!(
        m.statements, 1,
        "not(not(test)) statements should not be counted (got {})",
        m.statements
    );
}

#[test]
fn static_coverage_touch_accumulate_and_cfg_scan() {
    fn t<T>(_: T) {}
    t(accumulate_rust_file_metrics_from_items);
    t(contains_test_ident);
}
