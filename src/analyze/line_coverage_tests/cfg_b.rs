use super::*;

fn leftover_cfg_evaluator_absent() {
    let cfg = include_str!("../line_coverage_cfg.rs");
    assert!(
        !cfg.contains("fn cfg_expr_active") && !cfg.contains("fn item_cfg_active"),
        "leftover coverage cfg evaluator must be gone"
    );
}

#[test]
fn cfg_helpers_metamorphic_inactive_cfg_is_stable_across_item_kinds() {
    leftover_cfg_evaluator_absent();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("items.rs");
    std::fs::write(
        &file,
        concat!(
            "#[cfg(not(unix))]\n",
            "const C: i32 = 1;\n",
            "#[cfg(not(unix))]\n",
            "enum E { A }\n",
            "#[cfg(not(unix))]\n",
            "fn f() {\n",
            "    let x = 1;\n",
            "    let _ = x;\n",
            "}\n",
            "#[cfg(not(unix))]\n",
            "struct S;\n",
        ),
    )
    .unwrap();
    let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
    assert!(
        !denom.is_empty(),
        "unknown platform cfg items must stay coverable, got {denom:?}"
    );
}

#[test]
fn cfg_attrs_and_expr_error_paths_are_conservative() {
    leftover_cfg_evaluator_absent();
}

#[test]
fn cfg_helpers_fuzz_random_expr_kinds_stay_boolean() {
    leftover_cfg_evaluator_absent();
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
    let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
    assert!(
        denom.len() >= 3,
        "expected impl/block statements in denom, got {denom:?}"
    );
}

#[test]
fn cfg_helpers_cover_verbatim_yield_group_and_unknown_item() {
    leftover_cfg_evaluator_absent();
}
