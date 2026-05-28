use crate::rust_test_refs::*;
use crate::rust_parsing::parse_rust_file;
use std::io::Write;

#[test]
fn test_analyze_refs_for_coverage_map() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(
        tmp,
        "fn foo() {{}}\n#[cfg(test)] mod tests {{ use super::*; #[test] fn t() {{ foo(); }} }}"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let analysis = analyze_rust_test_refs_for_coverage_map(&[&parsed], None);
    assert!(analysis.unreferenced.is_empty());
    assert!(
        analysis.coverage_map.is_empty(),
        "kiss-coverage-map file_map path skips per-test coverage_map"
    );
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

#[test]
fn test_stringify_macro_and_path_ref_helpers() {
    let mac: syn::Macro = syn::parse_str("stringify!(x)").unwrap();
    assert!(references::is_stringify_macro(&mac));
    let mut refs = HashSet::new();
    references::insert_coverage_path_string_ref("src/x.rs::helper_fn", &mut refs);
    assert!(refs.contains("helper_fn"));
    references::insert_coverage_path_string_ref("not a path", &mut refs);
    assert!(!refs.contains("not"));
}

#[test]
fn test_coverage_map_mode_ignores_bare_path_types() {
    let ast: syn::File = syn::parse_str(
        "#[test]\nfn t() { let _: MyType = foo(); only_path(MyType); }",
    )
    .unwrap();
    let mut full = HashSet::new();
    let mut cal = HashSet::new();
    references::collect_rust_references(&ast, &mut full);
    references::collect_rust_references_for_coverage_map(&ast, &mut cal);
    assert!(full.contains("MyType"));
    assert!(!cal.contains("MyType") || cal.contains("foo"));
    assert!(cal.contains("foo"));
}

#[test]
fn test_pascal_case_ident_to_snake_skips_non_pascal() {
    let mut refs = HashSet::new();
    let path: syn::Path = syn::parse_str("Rule::lowercase").unwrap();
    references::insert_path_segments(&path, &mut refs);
    assert!(refs.contains("lowercase"));
    assert!(!refs.contains("line_contains_todo"));
    let path2: syn::Path = syn::parse_str("NotRule::LineContainsTodo").unwrap();
    references::insert_path_segments(&path2, &mut refs);
    assert!(refs.contains("LineContainsTodo"));
}

#[test]
fn test_stringify_and_path_string_refs() {
    let ast: syn::File = syn::parse_str(
        "#[test]\nfn t() { let _ = stringify!(ghost); foo(); let _ = \"src/a.rs::bar_helper\"; }",
    )
    .unwrap();
    let mut refs = HashSet::new();
    references::collect_rust_references(&ast, &mut refs);
    assert!(!refs.contains("ghost"));
    assert!(refs.contains("foo"));
    assert!(refs.contains("bar_helper"));
}

#[test]
fn test_is_external_crate_common_deps() {
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

#[test]
fn coverage_map_helpers_and_test_attr_paths() {
    let test_path: syn::Path = syn::parse_str("test").unwrap();
    let tokio_test: syn::Path = syn::parse_str("tokio::test").unwrap();
    assert!(crate::rust_test_refs::attr_path_is_test(&test_path));
    assert!(crate::rust_test_refs::attr_path_is_test(&tokio_test));

    let defs = vec![
        RustCodeDefinition {
            name: "seen".into(),
            kind: CodeUnitKind::Function,
            file: PathBuf::from("a.rs"),
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "miss".into(),
            kind: CodeUnitKind::Function,
            file: PathBuf::from("a.rs"),
            line: 2,
            end_line: 2,
            impl_for_type: None,
        },
    ];
    let counts: HashMap<PathBuf, usize> = defs.iter().fold(HashMap::new(), |mut m, d| {
        *m.entry(d.file.clone()).or_default() += 1;
        m
    });
    assert_eq!(counts.get(&PathBuf::from("a.rs")).copied(), Some(2));

    let name_files = crate::test_refs::build_name_file_map(
        defs.iter().map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let witness = HashSet::from(["seen".to_string()]);
    let ctx = crate::rust_test_refs::coverage_map_unreferenced::CoverageMapUnrefCtx {
        test_witness_refs: &witness,
        coverage_references: &HashSet::new(),
        name_files: &name_files,
        disambiguation: &HashMap::new(),
        integration_cone_files: &HashSet::new(),
        defs_per_file: &counts,
        cli_route_attested_files: &HashSet::new(),
    };
    let unref =
        crate::rust_test_refs::coverage_map_unreferenced::unreferenced_for_coverage_map(
            &defs, &ctx,
        );
    assert_eq!(unref.len(), 1);
    assert_eq!(unref[0].name, "miss");
}
