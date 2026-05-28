use super::calibration::module_definition_counts;
use super::coverage_map_collect;
use super::*;
use crate::graph::DependencyGraph;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_expand_witnessed_directory_skips_rule_settings() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rules = tmp
        .path()
        .join("crates/ruff_linter/src/rules/flake8_demo");
    std::fs::create_dir_all(&rules).unwrap();
    let settings = rules.join("settings.rs");
    let sibling = rules.join("mod.rs");
    std::fs::write(&settings, "pub struct DemoSettings;\n").unwrap();
    std::fs::write(&sibling, "pub fn run() {}\n").unwrap();
    let parsed_mod = parse_rust_file(&sibling).unwrap();
    let parsed_settings = parse_rust_file(&settings).unwrap();
    let refs_vec = [&parsed_mod, &parsed_settings];
    let mut refs = HashSet::from(["mod".to_string()]);
    super::calibration::expand_witnessed_directory_sibling_defs(&refs_vec, &mut refs);
    assert!(!refs.contains("DemoSettings"));
}

#[test]
fn test_expand_witnessed_directory_sibling_defs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("src/acp");
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("alpha.rs");
    let b = dir.join("beta.rs");
    std::fs::write(&a, "pub fn alpha() {}\n").unwrap();
    std::fs::write(&b, "pub fn beta() {}\n").unwrap();
    let parsed_a = parse_rust_file(&a).unwrap();
    let parsed_b = parse_rust_file(&b).unwrap();
    let refs_vec = [&parsed_a, &parsed_b];
    let mut refs = HashSet::from(["alpha".to_string()]);
    super::calibration::expand_witnessed_directory_sibling_defs(&refs_vec, &mut refs);
    assert!(refs.contains("beta"));
}

#[test]
fn test_module_definition_counts_uses_canonical_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lib = crate::rust_include::canonical_path(&tmp.path().join("lib.rs"));
    std::fs::write(&lib, "fn a() {}\nfn b() {}\n").unwrap();
    let mut graph = DependencyGraph::new();
    graph.path_to_module.insert(lib.clone(), "root".into());
    let parsed = parse_rust_file(&lib).unwrap();
    let refs = [&parsed];
    let analysis = analyze_rust_test_refs_for_coverage_map(&refs, Some(&graph));
    let counts = module_definition_counts(&analysis.definitions, &graph);
    assert_eq!(counts.get("root").copied(), Some(2));
}

#[test]
fn test_coverage_map_excludes_subprocess_test_witnesses() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(&lib, "pub fn only_in_unit() {}\n").unwrap();
    let unit = tmp.path().join("tests/unit.rs");
    std::fs::write(&unit, "#[test]\nfn ok() {}\n").unwrap();
    let integration = tmp.path().join("tests/spawn.rs");
    std::fs::write(
        &integration,
        "#[test]\nfn spawn() { std::process::Command::new(\"app\"); }\n",
    )
    .unwrap();
    let files: Vec<_> = [&lib, &unit, &integration]
        .into_iter()
        .map(|p| parse_rust_file(p).unwrap())
        .collect();
    let refs: Vec<_> = files.iter().collect();
    let analysis = analyze_rust_test_refs_for_coverage_map(&refs, None);
    assert!(
        analysis
            .unreferenced
            .iter()
            .any(|d| d.name == "only_in_unit"),
        "subprocess tests must not witness production fn"
    );
}

#[test]
fn test_subprocess_witness_helpers() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    let spawn = tmp.path().join("tests/spawn.rs");
    std::fs::write(
        &spawn,
        "#[test]\nfn s() { std::process::Command::new(\"a\"); }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&spawn).unwrap();
    let refs = [&parsed];
    let (_, _, _, usage) = coverage_map_collect::collect_coverage_map_scan(&refs);
    let subprocess = coverage_map_collect::subprocess_integration_test_paths(&refs);
    assert_eq!(subprocess.len(), 1);
    let witnesses =
        coverage_map_collect::test_witness_refs_excluding_subprocess(&usage, &subprocess);
    assert!(witnesses.is_empty(), "subprocess-only tests excluded");
}

#[test]
fn test_external_test_witness_refs_ignore_inline_cfg_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    std::fs::write(
        &lib,
        "#[cfg(test)]\nmod t {\n    #[test]\n    fn inline() { let _ = inline_sym(); }\n}\n",
    )
    .unwrap();
    let tests_dir = tmp.path().join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    std::fs::write(
        tests_dir.join("ext.rs"),
        "#[test]\nfn external() { let _ = external_sym(); }\n",
    )
    .unwrap();
    let lib_p = parse_rust_file(&lib).unwrap();
    let ext_p = parse_rust_file(&tests_dir.join("ext.rs")).unwrap();
    let refs = [&lib_p, &ext_p];
    let (_, _, _, usage) = coverage_map_collect::collect_coverage_map_scan(&refs);
    let subprocess = coverage_map_collect::subprocess_integration_test_paths(&refs);
    let external =
        coverage_map_collect::external_test_witness_refs(&usage, &subprocess);
    assert!(external.contains("external_sym"));
    assert!(!external.contains("inline_sym"));
}

#[test]
fn test_stem_disambiguation_via_is_directly_referenced() {
    let def = RustCodeDefinition {
        name: "widget".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("widget.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    };
    let other = RustCodeDefinition {
        name: "not_widget".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("widget.rs"),
        line: 2,
        end_line: 2,
        impl_for_type: None,
    };
    let refs = HashSet::from(["widget".to_string()]);
    let mut name_files = HashMap::new();
    name_files.insert(
        "widget".to_string(),
        HashSet::from([PathBuf::from("widget.rs"), PathBuf::from("other.rs")]),
    );
    assert!(is_directly_referenced(
        &def,
        &refs,
        &name_files,
        &HashMap::new()
    ));
    assert!(!is_directly_referenced(
        &other,
        &refs,
        &name_files,
        &HashMap::new()
    ));
}

#[test]
fn test_collect_coverage_map_scan_direct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(
        &lib,
        "pub fn scanned() {}\n#[cfg(test)] mod inner { pub fn touch() { scanned(); } }\n",
    )
    .unwrap();
    let smoke = tmp.path().join("cli_cross_cov_kiss.rs");
    std::fs::write(&smoke, "#[test]\nfn k() {}\n").unwrap();
    let parsed_lib = parse_rust_file(&lib).unwrap();
    let parsed_smoke = parse_rust_file(&smoke).unwrap();
    let refs: Vec<_> = [&parsed_lib, &parsed_smoke].into_iter().collect();
    let (defs, _test_refs, _cov_refs, usage) =
        coverage_map_collect::collect_coverage_map_scan(&refs);
    assert_eq!(defs.len(), 1);
    assert!(usage.is_empty(), "smoke file skipped for per_test_usage");
}

#[test]
fn test_integration_cone_file_paths_and_calibration_map() {
    use super::calibration_map::build_rust_coverage_map_for_calibration;
    use super::calibration::integration_cone_file_paths;

    let tmp = tempfile::TempDir::new().unwrap();
    let main_rs = tmp.path().join("main.rs");
    std::fs::write(&main_rs, "mod child;\nfn main() { child::run(); }\n").unwrap();
    let child_rs = tmp.path().join("child.rs");
    std::fs::write(&child_rs, "pub fn run() {}\n").unwrap();
    let parsed_main = parse_rust_file(&main_rs).unwrap();
    let parsed_child = parse_rust_file(&child_rs).unwrap();
    let files = [&parsed_main, &parsed_child];
    let seeds = vec![crate::rust_include::canonical_path(&main_rs)];
    let cone = integration_cone_file_paths(&files, &seeds, 4);
    assert!(cone.contains(&crate::rust_include::canonical_path(&child_rs)));

    let defs = vec![RustCodeDefinition {
        name: "run".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: child_rs,
        line: 1,
        end_line: 1,
        impl_for_type: None,
    }];
    let usage: PerTestUsage = Vec::new();
    let refs = HashSet::from(["run".to_string()]);
    let map = build_rust_coverage_map_for_calibration(
        &defs,
        &usage,
        &HashMap::new(),
        &HashMap::new(),
        &refs,
    );
    assert!(map.is_empty());
}

#[test]
fn test_coverage_map_skips_smoke_and_collects_production() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(
        &lib,
        "pub fn prod() {}\n#[cfg(test)] mod smoke { fn t() { prod(); } }\n",
    )
    .unwrap();
    let smoke = tmp.path().join("cli_cross_cov_kiss.rs");
    std::fs::write(&smoke, "#[test]\nfn smoke() { prod(); }\n").unwrap();
    let parsed_lib = parse_rust_file(&lib).unwrap();
    let parsed_smoke = parse_rust_file(&smoke).unwrap();
    let analysis = analyze_rust_test_refs_for_coverage_map(&[&parsed_lib, &parsed_smoke], None);
    assert!(
        analysis
            .definitions
            .iter()
            .any(|d| d.name == "prod"),
        "production defs collected"
    );
}
