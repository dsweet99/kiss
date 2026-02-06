use kiss::rust_graph::build_rust_dependency_graph;
use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use std::path::Path;

fn parse_rs(path: &Path) -> ParsedRustFile {
    parse_rust_file(path).expect("parse rust fixture")
}

#[test]
fn bug_rust_transitive_dependencies_should_not_count_external_imports() {
    // RULE: [Rust] [transitive_dependencies]
    //
    // Hypothesis: transitive dependency counts include external crates/stdlib paths, inflating coupling.
    // Prediction: rust_graph_ext_a has exactly 1 internal transitive dependency (rust_graph_ext_b).
    let a = parse_rs(Path::new("tests/fake_rust/rust_graph_ext_a.rs"));
    let b = parse_rs(Path::new("tests/fake_rust/rust_graph_ext_b.rs"));
    let parsed: Vec<&ParsedRustFile> = vec![&a, &b];
    let g = build_rust_dependency_graph(&parsed);

    let m = g.module_metrics("rust_graph_ext_a");
    assert_eq!(m.transitive_dependencies, 1);
}

