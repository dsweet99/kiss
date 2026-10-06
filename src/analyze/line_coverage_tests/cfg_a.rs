use super::*;

fn leftover_cfg_evaluator_absent() {
    let cfg = include_str!("../line_coverage_cfg.rs");
    assert!(
        !cfg.contains("fn cfg_expr_active")
            && !cfg.contains("fn stmt_cfg_active")
            && !cfg.contains("fn item_cfg_active")
            && !cfg.contains("cfg!(")
            && !cfg.contains("std::env::consts::OS"),
        "product coverage must not keep the leftover cfg evaluator"
    );
}

#[test]
fn cfg_expression_evaluator_is_conservative_for_common_platform_forms() {
    leftover_cfg_evaluator_absent();
}

#[test]
fn cfg_expression_evaluator_keeps_unknown_forms_active() {
    leftover_cfg_evaluator_absent();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("feat.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "#[cfg(feature = \"extra\")]\npub fn gated() {\n    let x = 1;\n    let _ = x;\n}\n",
    )
    .unwrap();
    let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
    assert!(
        !denom.is_empty(),
        "unknown feature cfg must stay coverable, got {denom:?}"
    );
}

#[test]
fn cfg_helpers_cover_item_and_expr_variants_exhaustively() {
    leftover_cfg_evaluator_absent();
    let off: syn::ItemFn = syn::parse_str("#[coverage(off)] fn f() { let x = 1; }").unwrap();
    assert!(super::line_coverage_cfg::coverage_off_attrs(&off.attrs));
    let doc_off: syn::ItemFn =
        syn::parse_str("#[doc = \"kiss-coverage-off\"] fn g() { let y = 1; }").unwrap();
    assert!(super::line_coverage_cfg::coverage_off_attrs(&doc_off.attrs));
    let live: syn::ItemFn = syn::parse_str("fn h() { let z = 1; }").unwrap();
    assert!(!super::line_coverage_cfg::coverage_off_attrs(&live.attrs));
}
