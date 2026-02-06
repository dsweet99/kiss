use kiss::graph::build_dependency_graph;
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::path::Path;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

#[test]
fn bug_transitive_dependencies_should_not_count_external_modules() {
    // RULE: [Python] [transitive_dependencies] counts downstream modules reachable in dependency graph.
    //
    // Hypothesis: transitive dependency counts include external imports (stdlib/3rd party),
    // inflating coupling metrics and breaking the intended meaning of module(file)-level coupling.
    //
    // Prediction: `tests.fake_python.graph_ext_a` should have exactly 1 internal transitive dependency:
    // `tests.fake_python.graph_ext_b`. External imports like `os`/`json` should not count.
    let a = parse_py(Path::new("tests/fake_python/graph_ext_a.py"));
    let b = parse_py(Path::new("tests/fake_python/graph_ext_b.py"));
    let parsed_files: Vec<&ParsedFile> = vec![&a, &b];
    let g = build_dependency_graph(&parsed_files);

    let m = g.module_metrics("tests.fake_python.graph_ext_a");
    assert_eq!(m.transitive_dependencies, 1);
}

