use kiss::config::Config;
use kiss::graph::{analyze_graph, build_dependency_graph};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

#[test]
fn bug_indirect_dependencies_should_not_count_external_modules() {
    // RULE: [Python] [indirect_dependencies]
    //
    // Hypothesis: dependency counts include external imports (stdlib/3rd party),
    // inflating coupling metrics.
    //
    // Prediction: `tests.fake_python.graph_ext_a` has 1 direct dep (graph_ext_b), 0 indirect.
    let a = parse_py(Path::new("tests/fake_python/graph_ext_a.py"));
    let b = parse_py(Path::new("tests/fake_python/graph_ext_b.py"));
    let parsed_files: Vec<&ParsedFile> = vec![&a, &b];
    let g = build_dependency_graph(&parsed_files);

    let m = g.module_metrics("tests.fake_python.graph_ext_a");
    assert_eq!(m.fan_out, 1);
    assert_eq!(m.indirect_dependencies, 0);
}

#[test]
fn bug_indirect_dependencies_violation_should_include_entry_modules() {
    // RULE: [Python] [indirect_dependencies]
    //
    // Hypothesis: `kiss check` suppresses indirect dependency violations for modules
    // with fan_in == 0, even though `kiss stats` includes those modules in its distribution.
    //
    // Prediction: An entry module with 1 indirect dependency and threshold 0 should emit an
    // `indirect_dependencies` violation.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("entry.py"), "import hub\n").unwrap();
    fs::write(root.join("hub.py"), "import leaf\n").unwrap();
    fs::write(root.join("leaf.py"), "VALUE = 1\n").unwrap();

    let entry = parse_py(&root.join("entry.py"));
    let hub = parse_py(&root.join("hub.py"));
    let leaf = parse_py(&root.join("leaf.py"));
    let parsed_files: Vec<&ParsedFile> = vec![&entry, &hub, &leaf];
    let g = build_dependency_graph(&parsed_files);

    let entry_module = g.module_for_path(&root.join("entry.py")).unwrap();
    let metrics = g.module_metrics(&entry_module);
    assert_eq!(metrics.fan_in, 0);
    assert_eq!(metrics.indirect_dependencies, 1);

    let config = Config {
        indirect_dependencies: 0,
        ..Config::python_defaults()
    };
    let violations = analyze_graph(&g, &config, false);

    assert!(
        violations
            .iter()
            .any(|v| v.metric == "indirect_dependencies" && v.unit_name == entry_module),
        "expected indirect_dependencies violation for entry module; got {violations:#?}"
    );
}
