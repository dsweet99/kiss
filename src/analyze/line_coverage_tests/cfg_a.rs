use super::line_coverage_cfg::{cfg_expr_active, literal_string_value};
use super::*;

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

    assert_eq!(record.total_lines, 2);
    assert_eq!(record.covered_lines, 2);
    assert_eq!(record.percent, 100);
}

#[test]
fn cfg_expression_evaluator_is_conservative_for_common_platform_forms() {
    assert_eq!(cfg_expr_active("unix".parse().unwrap()), Some(true));
    assert_eq!(cfg_expr_active("not(unix)".parse().unwrap()), Some(false));
    assert_eq!(
        cfg_expr_active("target_os = \"linux\"".parse().unwrap()),
        Some(true)
    );
    assert_eq!(
        cfg_expr_active("not(target_os = \"linux\")".parse().unwrap()),
        Some(false)
    );
    assert_eq!(
        cfg_expr_active("any(not(unix), target_os = \"linux\")".parse().unwrap()),
        Some(true)
    );
    assert_eq!(
        cfg_expr_active("all(unix, target_os = \"linux\")".parse().unwrap()),
        Some(true)
    );
}

#[test]
fn cfg_expression_evaluator_keeps_unknown_forms_active() {
    assert_eq!(
        cfg_expr_active("feature = \"extra\"".parse().unwrap()),
        None
    );
    assert_eq!(
        cfg_expr_active("any(not(unix), feature = \"extra\")".parse().unwrap()),
        None
    );
    assert_eq!(
        cfg_expr_active("all(unix, feature = \"extra\")".parse().unwrap()),
        None
    );
    assert_eq!(
        literal_string_value(&proc_macro2::Literal::string("other")),
        None
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

    let records = compute_line_coverage_records(
        tmp.path(),
        &[],
        &[lib, support.clone(), prod.clone()],
        &snapshot,
    );
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

    let records =
        compute_line_coverage_records(tmp.path(), &[], &[lib, maybe_support.clone()], &snapshot);
    let files = records
        .iter()
        .map(|record| record.file.clone())
        .collect::<BTreeSet<_>>();

    assert!(!files.contains(&maybe_support));
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

    let records = compute_line_coverage_records(
        tmp.path(),
        &[],
        &[lib, mod_file.clone(), child.clone()],
        &snapshot,
    );
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

    let records = compute_line_coverage_records(
        tmp.path(),
        &[],
        &[lib, prod, mod_file.clone(), child.clone()],
        &snapshot,
    );
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

    let test_only = cfg_test_only_rust_files(&[missing, malformed]);

    assert!(test_only.is_empty());
}

#[test]
fn cfg_helpers_cover_item_and_expr_variants_exhaustively() {
    use super::line_coverage_cfg::{expr_cfg_active, item_cfg_active, stmt_cfg_active};

    let item_snippets = [
        "const C: i32 = 1;",
        "enum E { A }",
        "extern crate std;",
        "fn f() {}",
        "extern \"C\" { fn g(); }",
        "impl Foo { fn h(&self) {} }",
        "macro_rules! m { () => {}; }",
        "mod nested {}",
        "static S: i32 = 1;",
        "struct St;",
        "trait Tr {}",
        "trait Alias = Tr;",
        "type T = i32;",
        "union U { x: i32 }",
        "use std::collections::BTreeSet;",
    ];
    for snippet in item_snippets {
        let item: syn::Item = syn::parse_str(snippet).unwrap();
        assert!(item_cfg_active(&item), "item should be active: {snippet}");
        let stmt = syn::Stmt::Item(item);
        assert!(
            stmt_cfg_active(&stmt),
            "stmt item should be active: {snippet}"
        );
    }

    let inactive: syn::Item = syn::parse_str("#[cfg(not(unix))] struct Off;").unwrap();
    assert!(!item_cfg_active(&inactive));

    let expr_snippets = [
        "[1, 2]",
        "x = 1",
        "async { 1 }",
        "fut.await",
        "1 + 2",
        "{ 1 }",
        "break",
        "f()",
        "1 as i32",
        "|x| x",
        "const { 1 }",
        "continue",
        "s.field",
        "for x in xs { let _ = x; }",
        "(1)",
        "if true { 1 } else { 0 }",
        "xs[0]",
        "_",
        "let Some(x) = opt",
        "1",
        "loop { break; }",
        "println!(\"x\")",
        "match x { _ => 1 }",
        "x.method()",
        "(1 + 2)",
        "path::to::Value",
        "0..1",
        "&x",
        "[0; 2]",
        "return",
        "Point { x: 1 }",
        "falliable?",
        "try { 1 }",
        "(1, 2)",
        "-1",
        "unsafe { 1 }",
        "while false {}",
        // yield needs nightly in some contexts; skip if parse fails
    ];
    let mut parsed = 0usize;
    for snippet in expr_snippets {
        let Ok(expr) = syn::parse_str::<syn::Expr>(snippet) else {
            continue;
        };
        parsed += 1;
        assert!(expr_cfg_active(&expr), "expr should be active: {snippet}");
        let stmt = syn::Stmt::Expr(expr, None);
        assert!(
            stmt_cfg_active(&stmt),
            "stmt expr should be active: {snippet}"
        );
    }
    assert!(
        parsed >= 30,
        "expected most expr snippets to parse, got {parsed}"
    );

    let local: syn::Stmt = syn::parse_str("let x = 1;").unwrap();
    assert!(stmt_cfg_active(&local));
    let mac: syn::Stmt = syn::parse_str("println!(\"hi\");").unwrap();
    assert!(stmt_cfg_active(&mac));
}
