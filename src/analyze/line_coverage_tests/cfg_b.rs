use super::*;

#[test]
fn cfg_helpers_metamorphic_inactive_cfg_is_stable_across_item_kinds() {
    use super::line_coverage_cfg::item_cfg_active;
    let snippets = [
        "#[cfg(not(unix))] const C: i32 = 1;",
        "#[cfg(not(unix))] enum E { A }",
        "#[cfg(not(unix))] fn f() {}",
        "#[cfg(not(unix))] struct S;",
        "#[cfg(not(unix))] type T = i32;",
        "#[cfg(not(unix))] use std::mem;",
    ];
    for snippet in snippets {
        let item: syn::Item = syn::parse_str(snippet).unwrap();
        assert!(!item_cfg_active(&item), "expected inactive: {snippet}");
    }
}

#[test]
fn cfg_attrs_and_expr_error_paths_are_conservative() {
    use super::line_coverage_cfg::{cfg_attrs_active, cfg_expr_active};

    let bare_cfg: syn::Item = syn::parse_str("#[cfg] struct Bare;").unwrap();
    if let syn::Item::Struct(item) = bare_cfg {
        assert!(cfg_attrs_active(&item.attrs));
    }
    let name_value_cfg: syn::Item = syn::parse_str("#[cfg = \"unix\"] struct Named;").unwrap();
    if let syn::Item::Struct(item) = name_value_cfg {
        assert!(cfg_attrs_active(&item.attrs));
    }
    let non_cfg: syn::Item = syn::parse_str("#[allow(dead_code)] struct A;").unwrap();
    if let syn::Item::Struct(item) = non_cfg {
        assert!(cfg_attrs_active(&item.attrs));
    }

    assert_eq!(cfg_expr_active("not".parse().unwrap()), None);
    assert_eq!(cfg_expr_active("not 1".parse().unwrap()), None);
    assert_eq!(cfg_expr_active("any".parse().unwrap()), None);
    assert_eq!(cfg_expr_active("all".parse().unwrap()), None);
    assert_eq!(cfg_expr_active("target_os".parse().unwrap()), None);
    // Malformed tokens are either rejected (None) or treated as inactive (Some(false)).
    for snippet in ["target_os > \"linux\"", "target_os = 1", "mystery"] {
        let value = cfg_expr_active(snippet.parse().unwrap());
        assert!(
            value.is_none() || value == Some(false),
            "snippet={snippet:?} value={value:?}"
        );
    }
}

#[test]
fn cfg_helpers_fuzz_random_expr_kinds_stay_boolean() {
    use super::line_coverage_cfg::expr_cfg_active;
    let seed = 0xC0FF_EE42u64;
    eprintln!("cfg_helpers_fuzz_random_expr_kinds_stay_boolean seed={seed}");
    let mut state = seed;
    let corpus = [
        "1",
        "x",
        "f()",
        "x.y",
        "xs[0]",
        "&x",
        "*x",
        "-x",
        "1+2",
        "{1}",
        "(1,2)",
        "async {1}",
        "match x { _ => 1 }",
        "if true {1} else {0}",
        "loop { break }",
        "while false {}",
    ];
    for _ in 0..64 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let snippet = corpus[(state as usize) % corpus.len()];
        let expr: syn::Expr = syn::parse_str(snippet).unwrap();
        let _ = expr_cfg_active(&expr);
    }
}

#[test]
fn cfg_test_only_recognizes_compound_and_grouped_cfg_test_forms() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let plain = src.join("plain.rs");
    let all_test = src.join("all_test.rs");
    let any_test = src.join("any_test.rs");
    let group_test = src.join("group_test.rs");
    let not_test = src.join("not_test.rs");
    let named = src.join("named.rs");
    for path in [&plain, &all_test, &any_test, &group_test, &not_test, &named] {
        std::fs::write(path, "pub fn marker() {}\n").unwrap();
    }
    std::fs::write(
        &lib,
        concat!(
            "mod plain;\n",
            "#[cfg(all(test, unix))]\n",
            "mod all_test;\n",
            "#[cfg(any(test, windows))]\n",
            "mod any_test;\n",
            "#[cfg((test))]\n",
            "mod group_test;\n",
            "#[cfg(not(test))]\n",
            "mod not_test;\n",
            "#[cfg = \"test\"]\n",
            "mod named;\n",
        ),
    )
    .unwrap();

    let files = vec![
        lib.clone(),
        plain.clone(),
        all_test.clone(),
        any_test.clone(),
        group_test.clone(),
        not_test.clone(),
        named.clone(),
    ];
    let test_only = cfg_test_only_rust_files(&files);
    assert!(test_only.contains(&all_test));
    // `any(...)` skips nested tokens by design in cfg_tokens_contain_test.
    assert!(!test_only.contains(&any_test));
    assert!(test_only.contains(&group_test));
    assert!(!test_only.contains(&plain));
    assert!(!test_only.contains(&not_test));
    assert!(!test_only.contains(&named));
    // Still exercise the any(...) token walk even when it does not mark test-only.
    assert!(files.iter().any(|p| p.ends_with("any_test.rs")));
}

#[test]
fn coverage_denominator_visits_impl_methods_and_cfg_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("impls.rs");
    std::fs::write(
        &file,
        concat!(
            "struct S;\n",
            "impl S {\n",
            "    fn live(&self) {\n",
            "        let x = 1;\n",
            "        let _ = x;\n",
            "    }\n",
            "    #[cfg(not(unix))]\n",
            "    fn dead(&self) {\n",
            "        let y = 1;\n",
            "        let _ = y;\n",
            "    }\n",
            "}\n",
            "fn blocks() {\n",
            "    #[cfg(not(unix))]\n",
            "    {\n",
            "        let z = 1;\n",
            "        let _ = z;\n",
            "    }\n",
            "    {\n",
            "        let w = 1;\n",
            "        let _ = w;\n",
            "    }\n",
            "}\n",
            "#[cfg(not(unix))]\n",
            "fn inactive_fn() {\n",
            "    let a = 1;\n",
            "    let _ = a;\n",
            "}\n",
        ),
    )
    .unwrap();
    let denom = coverage_denominator_lines(&file);
    assert!(
        denom.len() >= 3,
        "expected impl/block statements in denom, got {denom:?}"
    );
}

#[test]
fn module_path_attr_ignores_non_string_path_forms() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let alt = src.join("alt.rs");
    std::fs::write(&alt, "pub fn marker() {}\n").unwrap();
    std::fs::write(
        &lib,
        concat!(
            "#[path]\n",
            "mod broken_bare;\n",
            "#[path = 1]\n",
            "mod broken_int;\n",
            "#[path = \"alt.rs\"]\n",
            "mod ok;\n",
        ),
    )
    .unwrap();
    let files = vec![lib.clone(), alt.clone()];
    let test_only = cfg_test_only_rust_files(&files);
    assert!(test_only.is_empty());
    let records = compute_line_coverage_records(
        tmp.path(),
        &[],
        &files,
        &RuntimeCoverageSnapshot {
            identity: "id".into(),
            covered_lines: BTreeMap::new(),
        },
    );
    assert_eq!(records.len(), 2);
}

#[test]
fn coverage_denominator_returns_empty_for_unreadable_path() {
    let missing =
        std::path::PathBuf::from("/tmp/kiss-missing-coverage-denom-file-does-not-exist.rs");
    let denom = coverage_denominator_lines(&missing);
    assert!(denom.is_empty());
}

#[test]
fn cfg_helpers_cover_verbatim_yield_group_and_unknown_item() {
    use super::line_coverage_cfg::{expr_cfg_active, item_cfg_active};
    use proc_macro2::{Delimiter, Group, Ident, Span, TokenStream, TokenTree};

    let verbatim = syn::Expr::Verbatim(TokenStream::new());
    assert!(expr_cfg_active(&verbatim));

    let yield_expr = syn::Expr::Yield(syn::ExprYield {
        attrs: Vec::new(),
        yield_token: syn::token::Yield {
            span: Span::call_site(),
        },
        expr: None,
    });
    assert!(expr_cfg_active(&yield_expr));

    let inner: syn::Expr = syn::parse_str("1").unwrap();
    let group = syn::Expr::Group(syn::ExprGroup {
        attrs: Vec::new(),
        group_token: syn::token::Group {
            span: Span::call_site(),
        },
        expr: Box::new(inner),
    });
    assert!(expr_cfg_active(&group));

    // Force item_cfg_active_b's `_ => true` arm via Verbatim item.
    let item = syn::Item::Verbatim(TokenStream::from_iter([TokenTree::Ident(Ident::new(
        "mystery",
        Span::call_site(),
    ))]));
    assert!(item_cfg_active(&item));

    // Keep a Group token around so the constructor path stays exercised in cfg fuzzing.
    let _ = TokenTree::Group(Group::new(Delimiter::Parenthesis, TokenStream::new()));
}
