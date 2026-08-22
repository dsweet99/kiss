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
fn rust_coverage_denominator_ignores_inactive_cfg_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("platform.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "pub fn ensure_supported() -> Result<(), &'static str> {\n\
                 #[cfg(not(target_os = \"linux\"))]\n\
                 {\n\
                     return Err(\"linux required\");\n\
                 }\n\
                 #[cfg(not(unix))]\n\
                 {\n\
                     return Err(\"unix required\");\n\
                 }\n\
                 Ok(())\n\
             }\n",
    )
    .unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::from([("src/platform.rs".to_string(), BTreeSet::from([1, 10]))]),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert!(
        record.total_lines > 2,
        "unknown platform cfg must stay coverable, got {record:?}"
    );
    assert!(record.percent < 100);
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
fn runtime_records_skip_cfg_test_only_rust_modules() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let support = src.join("support.rs");
    let prod = src.join("batch_plan_test_args.rs");
    std::fs::write(
        &lib,
        "#[cfg(test)]\nmod support;\nmod batch_plan_test_args;\n",
    )
    .unwrap();
    std::fs::write(&support, "pub fn helper() {\n    println!(\"test\");\n}\n").unwrap();
    std::fs::write(&prod, "pub fn plan() {\n    println!(\"prod\");\n}\n").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };

    let records = compute_line_coverage_records_for_test(
        tmp.path(),
        &[],
        &[lib, support.clone(), prod.clone()],
        &snapshot,
    )
    .unwrap();
    let files = records
        .iter()
        .map(|record| record.file.clone())
        .collect::<BTreeSet<_>>();

    assert!(!files.contains(&support));
    assert!(files.contains(&prod));
}

#[test]
fn cfg_any_test_feature_module_is_test_only() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    let maybe_support = src.join("maybe_support.rs");
    std::fs::write(
        &lib,
        "#[cfg(any(test, feature = \"extra\"))]\nmod maybe_support;\n",
    )
    .unwrap();
    std::fs::write(&maybe_support, "pub fn helper() {}\n").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };

    let records = compute_line_coverage_records_for_test(
        tmp.path(),
        &[],
        &[lib, maybe_support.clone()],
        &snapshot,
    )
    .unwrap();
    let files = records
        .iter()
        .map(|record| record.file.clone())
        .collect::<BTreeSet<_>>();

    assert!(files.contains(&maybe_support));
}

#[test]
fn runtime_records_skip_children_of_cfg_test_only_rust_modules() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let fixtures = src.join("fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    let lib = src.join("lib.rs");
    let mod_file = fixtures.join("mod.rs");
    let child = fixtures.join("child.rs");
    std::fs::write(&lib, "#[cfg(test)]\nmod fixtures;\n").unwrap();
    std::fs::write(&mod_file, "mod child;\n").unwrap();
    std::fs::write(&child, "pub fn helper() {}\n").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };

    let records = compute_line_coverage_records_for_test(
        tmp.path(),
        &[],
        &[lib, mod_file.clone(), child.clone()],
        &snapshot,
    )
    .unwrap();
    let files = records
        .iter()
        .map(|record| record.file.clone())
        .collect::<BTreeSet<_>>();

    assert!(!files.contains(&mod_file));
    assert!(!files.contains(&child));
}

#[test]
fn production_reference_keeps_shared_child_module_eligible() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let fixtures = src.join("fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    let lib = src.join("lib.rs");
    let prod = src.join("prod.rs");
    let mod_file = fixtures.join("mod.rs");
    let child = fixtures.join("child.rs");
    std::fs::write(&lib, "#[cfg(test)]\nmod fixtures;\nmod prod;\n").unwrap();
    std::fs::write(&prod, "#[path = \"fixtures/child.rs\"]\nmod shared;\n").unwrap();
    std::fs::write(&mod_file, "mod child;\n").unwrap();
    std::fs::write(&child, "pub fn helper() {}\n").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };

    let records = compute_line_coverage_records_for_test(
        tmp.path(),
        &[],
        &[lib, prod, mod_file.clone(), child.clone()],
        &snapshot,
    )
    .unwrap();
    let files = records
        .iter()
        .map(|record| record.file.clone())
        .collect::<BTreeSet<_>>();

    assert!(!files.contains(&mod_file));
    assert!(files.contains(&child));
}

#[test]
fn cfg_test_only_scan_tolerates_missing_and_malformed_rust_files() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let missing = src.join("missing.rs");
    let malformed = src.join("malformed.rs");
    std::fs::write(&malformed, "mod nope {\n").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };
    assert!(
        compute_line_coverage_records_for_test(tmp.path(), &[], &[malformed], &snapshot).is_err()
    );
    assert!(
        compute_line_coverage_records_for_test(tmp.path(), &[], &[missing], &snapshot).is_err()
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
