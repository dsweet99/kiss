use super::*;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[test]
fn test_platform_gated_coverage_helpers() {
    assert!(coverage::is_platform_specific_prod_file(&PathBuf::from(
        "src/foo_win32.rs"
    )));
    assert!(coverage::is_platform_specific_prod_file(&PathBuf::from(
        "rich/_windows_renderer.py"
    )));
    assert!(!coverage::is_platform_specific_prod_file(&PathBuf::from("src/foo.rs")));
    assert!(coverage::is_windows_gated_test_file(
        "import sys\nif sys.platform != \"win32\":\n    pass\n"
    ));
    assert!(!coverage::is_windows_gated_test_file("def test_x(): pass\n"));
}

#[test]
fn test_deprioritize_platform_gated_coverage() {
    use crate::parsing::{create_parser, parse_file};
    let mut prod = NamedTempFile::with_suffix("_win32.py").unwrap();
    write!(prod, "def api():\n    pass\n").unwrap();
    let mut gated_test = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(
        gated_test,
        "import sys\nif sys.platform != 'win32':\n    def test_api():\n        api()\n"
    )
    .unwrap();
    let mut clean_test = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(clean_test, "def test_other():\n    pass\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let prod_p = parse_file(&mut parser, prod.path()).expect("parse");
    let gated_p = parse_file(&mut parser, gated_test.path()).expect("parse");
    let clean_p = parse_file(&mut parser, clean_test.path()).expect("parse");
    let parsed = [&prod_p, &gated_p, &clean_p];
    let analysis = analyze_test_refs_for_coverage_map(&parsed, None);
    let mut unreferenced = analysis.unreferenced;
    coverage::deprioritize_platform_gated_coverage(
        &analysis.definitions,
        &mut unreferenced,
        &Vec::new(),
        &parsed,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );
}

#[test]
fn test_analyze_coverage_map_without_graph_skips_import_calibration() {
    use crate::parsing::{create_parser, parse_file};
    let mut lib = NamedTempFile::with_suffix(".py").unwrap();
    write!(lib, "def api():\n    pass\n").unwrap();
    let mut testf = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(testf, "def test_api():\n    api()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let lib_p = parse_file(&mut parser, lib.path()).expect("parse");
    let test_p = parse_file(&mut parser, testf.path()).expect("parse");
    let analysis = analyze_test_refs_for_coverage_map(&[&lib_p, &test_p], None);
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn test_analyze_test_refs_quick_and_no_map_empty() {
    use super::TestRefsAnalysisKind;
    let analysis = analyze_test_refs_quick(&[]);
    assert!(analysis.definitions.is_empty());
    let no_map = analyze_test_refs_no_map(&[], None);
    assert!(no_map.coverage_map.is_empty());
    assert!(matches!(
        TestRefsAnalysisKind::Full {
            need_coverage_map: true
        },
        TestRefsAnalysisKind::Full {
            need_coverage_map: true
        }
    ));
    assert!(matches!(
        TestRefsAnalysisKind::CoverageCalibration,
        TestRefsAnalysisKind::CoverageCalibration
    ));
}

#[test]
fn test_collect_all_test_file_data_coverage_map_omits_bare_identifiers() {
    use crate::parsing::{create_parser, parse_file};
    use super::collect::{
        collect_all_test_file_data, collect_all_test_file_data_for_coverage_map,
    };
    let mut testf = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(
        testf,
        "def test_x():\n    bare_name\n    real_call()\ndef real_call():\n    pass\n"
    )
    .unwrap();
    let mut parser = create_parser().expect("parser");
    let parsed = parse_file(&mut parser, testf.path()).expect("parse");
    let root = parsed.tree.root_node();
    let mut full_usage = HashSet::new();
    let mut cal_usage = HashSet::new();
    let mut t = HashSet::new();
    let mut bindings = HashMap::new();
    collect_all_test_file_data(root, &parsed.source, &mut t, &mut full_usage, &mut bindings);
    collect_all_test_file_data_for_coverage_map(
        root,
        &parsed.source,
        &mut t,
        &mut cal_usage,
        &mut bindings,
    );
    assert!(full_usage.contains("bare_name"));
    assert!(!cal_usage.contains("bare_name"));
    assert!(cal_usage.contains("real_call"));
}

#[test]
fn test_analyze_test_refs_for_coverage_map_fixture() {
    use crate::graph::build_dependency_graph;
    use crate::parsing::{create_parser, parse_file};
    let mut lib = NamedTempFile::with_suffix(".py").unwrap();
    write!(lib, "def api():\n    helper()\ndef helper():\n    pass\n").unwrap();
    let mut testf = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(testf, "def test_api():\n    api()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let lib_p = parse_file(&mut parser, lib.path()).expect("parse");
    let test_p = parse_file(&mut parser, testf.path()).expect("parse");
    let refs = [&lib_p, &test_p];
    let graph = build_dependency_graph(&refs);
    let analysis = analyze_test_refs_for_coverage_map(&refs, Some(&graph));
    assert!(!analysis.definitions.is_empty());
    assert!(
        analysis
            .unreferenced
            .iter()
            .all(|d| d.name != "api" && d.name != "helper"),
        "calibration witnesses should cover api and helper"
    );
}

#[test]
fn test_collect_refs_parallel_for_coverage_map_paths() {
    use crate::parsing::{create_parser, parse_file};
    let mut lib = NamedTempFile::with_suffix(".py").unwrap();
    write!(lib, "def api():\n    helper()\ndef helper():\n    pass\n").unwrap();
    let mut testf = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(testf, "def test_api():\n    api()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let lib_p = parse_file(&mut parser, lib.path()).expect("parse");
    let test_p = parse_file(&mut parser, testf.path()).expect("parse");
    let files = [&lib_p, &test_p];
    let (defs, _, usage, _, _) = collect_refs_parallel_for_coverage_map(&files);
    assert!(!defs.is_empty());
    assert!(usage.contains("api"));
    let (_, _, usage2, _, pt2) = collect_refs_parallel(&files, true);
    assert!(usage2.contains("api"));
    assert!(!pt2.is_empty());
    let (_, _, usage3, _, pt3) = collect_refs_parallel(&files, false);
    assert!(usage3.contains("api"));
    assert!(pt3.is_empty());
    let (defs_only, _, _, _, _) = collect_refs_parallel_for_coverage_map(&[&lib_p]);
    assert!(!defs_only.is_empty());
}

#[test]
fn test_is_definition_covered_for_calibration_rejects_class_only() {
    let def = CodeDefinition {
        name: "process".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("mod.py"),
        line: 5,
        end_line: 8,
        containing_class: Some("MyClass".into()),
    };
    let empty_map: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    let empty_mod: HashMap<PathBuf, String> = HashMap::new();
    let empty_import: HashMap<String, HashSet<String>> = HashMap::new();
    let mut usage = HashSet::new();
    usage.insert("MyClass".into());
    assert!(!is_definition_covered_for_calibration(
        &def,
        &empty_map,
        &HashMap::new(),
        &empty_import,
        &empty_mod,
        &usage,
    ));
    usage.insert("process".into());
    assert!(is_definition_covered_for_calibration(
        &def,
        &empty_map,
        &HashMap::new(),
        &empty_import,
        &empty_mod,
        &usage,
    ));
}

#[test]
fn test_apply_import_dependency_calibration_requires_module_witness() {
    use crate::graph::DependencyGraph;
    let main_path = PathBuf::from("/proj/main.py");
    let helper_path = PathBuf::from("/proj/helper.py");
    let api_def = CodeDefinition {
        name: "api".into(),
        kind: CodeUnitKind::Function,
        file: main_path.clone(),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let helper_def = CodeDefinition {
        name: "helper_only".into(),
        kind: CodeUnitKind::Function,
        file: helper_path.clone(),
        line: 1,
        end_line: 2,
        containing_class: None,
    };
    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(main_path, "main".to_string());
    graph
        .path_to_module
        .insert(helper_path, "helper".to_string());
    graph.add_dependency("main", "helper");
    let mut analysis = TestRefAnalysis {
        definitions: vec![api_def, helper_def.clone()],
        test_references: HashSet::new(),
        unreferenced: vec![helper_def],
        coverage_map: HashMap::new(),
    };
    let mut usage = HashSet::new();
    usage.insert("api".to_string());
    coverage::apply_import_dependency_calibration(&mut analysis, &graph, &usage);
    assert_eq!(analysis.unreferenced.len(), 1);
    usage.insert("helper_only".to_string());
    analysis.unreferenced.clear();
    coverage::apply_import_dependency_calibration(&mut analysis, &graph, &usage);
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn test_apply_import_dependency_calibration_unknown_module() {
    use crate::graph::DependencyGraph;
    let mut analysis = TestRefAnalysis {
        definitions: vec![CodeDefinition {
            name: "orphan".into(),
            kind: CodeUnitKind::Function,
            file: PathBuf::from("/orphan.py"),
            line: 1,
            end_line: 1,
            containing_class: None,
        }],
        test_references: HashSet::new(),
        unreferenced: vec![CodeDefinition {
            name: "orphan".into(),
            kind: CodeUnitKind::Function,
            file: PathBuf::from("/orphan.py"),
            line: 1,
            end_line: 1,
            containing_class: None,
        }],
        coverage_map: HashMap::new(),
    };
    let dep = DependencyGraph::new();
    coverage::apply_import_dependency_calibration(&mut analysis, &dep, &HashSet::new());
    assert_eq!(analysis.unreferenced.len(), 1);
}

#[test]
fn test_module_definition_counts_from_graph() {
    use crate::graph::DependencyGraph;
    let path = PathBuf::from("/proj/mod.py");
    let def = CodeDefinition {
        name: "f".into(),
        kind: CodeUnitKind::Function,
        file: path.clone(),
        line: 1,
        end_line: 1,
        containing_class: None,
    };
    let mut graph = DependencyGraph::new();
    graph.path_to_module.insert(path, "mod".into());
    let counts = coverage::module_definition_counts(&[def], &graph);
    assert_eq!(counts.get("mod"), Some(&1));
    assert!(coverage::module_definition_counts(&[], &graph).is_empty());
}

#[test]
fn test_collect_definitions_skips_dunder_main_block() {
    use crate::parsing::{create_parser, parse_file};
    use super::collect_definitions;
    use std::path::Path;
    let mut parser = create_parser().expect("parser");
    let src = "class Prod:\n    pass\nif __name__ == \"__main__\":\n    class Demo:\n        pass\n";
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").expect("tmp");
    std::io::Write::write_all(&mut tmp, src.as_bytes()).expect("write");
    let parsed = parse_file(&mut parser, tmp.path()).expect("parse");
    let mut defs = Vec::new();
    collect_definitions(
        parsed.tree.root_node(),
        &parsed.source,
        Path::new("mod.py"),
        &mut defs,
        false,
        None,
    );
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"Prod"));
    assert!(!names.contains(&"Demo"));
}

#[test]
fn test_is_dunder_main_guard_negative_cases() {
    use crate::parsing::create_parser;
    use super::collect::is_dunder_main_guard;
    let mut parser = create_parser().expect("parser");
    for src in [
        "if x == 1:\n    pass\n",
        "if __name__ != \"__main__\":\n    pass\n",
    ] {
        let tree = parser.parse(src, None).expect("parse");
        let if_node = tree.root_node().child(0).expect("if");
        assert!(!is_dunder_main_guard(if_node, src));
    }
    let src = "if __name__ == \"__main__\":\n    pass\n";
    let tree = parser.parse(src, None).expect("parse");
    let if_node = tree.root_node().child(0).expect("if");
    assert!(is_dunder_main_guard(if_node, src));
}

#[test]
fn test_deprioritize_class_name_witness_in_non_gated_test() {
    use crate::parsing::{create_parser, parse_file};
    let mut prod = NamedTempFile::with_suffix("_windows.py").unwrap();
    write!(prod, "class Win:\n    def api(self):\n        pass\n").unwrap();
    let mut gated = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(
        gated,
        "import sys\nif sys.platform != 'win32':\n    pass\n"
    )
    .unwrap();
    let mut direct = NamedTempFile::with_suffix("_test.py").unwrap();
    write!(direct, "def test_win():\n    Win()\n").unwrap();
    let mut parser = create_parser().expect("parser");
    let prod_p = parse_file(&mut parser, prod.path()).expect("parse");
    let gated_p = parse_file(&mut parser, gated.path()).expect("parse");
    let direct_p = parse_file(&mut parser, direct.path()).expect("parse");
    let parsed = [&prod_p, &gated_p, &direct_p];
    let analysis = analyze_test_refs_for_coverage_map(&parsed, None);
    let mut unreferenced = analysis.unreferenced.clone();
    let per_test = super::collect_parallel::collect_refs_parallel_for_coverage_map(&parsed).4;
    coverage::deprioritize_platform_gated_coverage(
        &analysis.definitions,
        &mut unreferenced,
        &per_test,
        &parsed,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );
    assert!(
        !unreferenced.iter().any(|d| d.name == "api"),
        "class-name witness in non-gated test should keep method covered"
    );
}
