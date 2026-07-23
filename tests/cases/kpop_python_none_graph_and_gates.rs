use kiss::graph::build_dependency_graph;
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::path::Path;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

#[test]
fn kpop_python_none_cycle_size() {
    // RULE: cycle_size
    let cycle_a = parse_py(Path::new("tests/fake_python/kpop_graph/cycle_a.py"));
    let cycle_b = parse_py(Path::new("tests/fake_python/kpop_graph/cycle_b.py"));
    let cycle_c = parse_py(Path::new("tests/fake_python/kpop_graph/cycle_c.py"));
    let parsed: Vec<&ParsedFile> = vec![&cycle_a, &cycle_b, &cycle_c];
    let graph = build_dependency_graph(&parsed);
    let cycles = graph.find_cycles().cycles;
    assert!(!cycles.is_empty());
    let any3 = cycles.iter().any(|cyc| cyc.len() == 3);
    assert!(any3, "cycles: {cycles:?}");
}

#[test]
fn kpop_python_none_dependency_depth() {
    // RULE: dependency_depth
    let chain_a = parse_py(Path::new("tests/fake_python/kpop_graph/chain_a.py"));
    let chain_b = parse_py(Path::new("tests/fake_python/kpop_graph/chain_b.py"));
    let chain_c = parse_py(Path::new("tests/fake_python/kpop_graph/chain_c.py"));
    let chain_d = parse_py(Path::new("tests/fake_python/kpop_graph/chain_d.py"));
    let parsed: Vec<&ParsedFile> = vec![&chain_a, &chain_b, &chain_c, &chain_d];
    let graph = build_dependency_graph(&parsed);

    let metrics = graph.module_metrics("tests.fake_python.kpop_graph.chain_a");
    assert!(
        metrics.dependency_depth >= 3,
        "depth={}",
        metrics.dependency_depth
    );
}

#[test]
fn kpop_python_none_test_coverage_threshold() {
    // RULE: test_coverage_threshold
    //
    // Static-reference coverage was removed. Runtime coverage is owned by
    // `kiss cov`; this test only asserts the threshold config remains loadable
    // and the static-reference APIs are gone.
    let gate = kiss::GateConfig {
        test_coverage_threshold: 90,
        ..Default::default()
    };
    assert_eq!(gate.test_coverage_threshold, 90);
    assert!(
        !std::fs::read_to_string("src/lib.rs")
            .unwrap()
            .contains("analyze_test_refs"),
        "static-reference analyze_test_refs must not be re-exported"
    );
}

#[test]
fn kpop_python_none_min_similarity() {
    // RULE: min_similarity
    //
    // KPOP hypothesis: detect_duplicates reports highly similar blocks.
    // We assert that obvious duplication yields at least one cluster.
    let p = parse_py(Path::new("tests/fake_python/user_service.py"));
    let parsed: Vec<&ParsedFile> = vec![&p];
    let dups = kiss::detect_duplicates(&parsed, &kiss::DuplicationConfig::default());
    assert!(!dups.is_empty());
    assert!(dups[0].similarity >= 0.9);
}
