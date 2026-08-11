use kiss::rust_graph::build_rust_dependency_graph;
use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use std::path::Path;

fn parse_rs(path: &Path) -> ParsedRustFile {
    parse_rust_file(path).expect("parse rust fixture")
}

#[test]
fn kpop_rust_none_cycle_size() {
    // RULE: cycle_size
    let cycle_a = parse_rs(Path::new("tests/fake_rust/kpop_graph/cycle_a.rs"));
    let cycle_b = parse_rs(Path::new("tests/fake_rust/kpop_graph/cycle_b.rs"));
    let cycle_c = parse_rs(Path::new("tests/fake_rust/kpop_graph/cycle_c.rs"));
    let parsed: Vec<&ParsedRustFile> = vec![&cycle_a, &cycle_b, &cycle_c];
    let graph = build_rust_dependency_graph(&parsed);
    let cycles = graph.find_cycles().cycles;
    assert!(!cycles.is_empty());
    assert!(
        cycles.iter().any(|cyc| cyc.len() == 3),
        "cycles: {cycles:?}"
    );
}

#[test]
fn kpop_rust_none_dependency_depth() {
    // RULE: dependency_depth
    let chain_a = parse_rs(Path::new("tests/fake_rust/kpop_graph/chain_a.rs"));
    let chain_b = parse_rs(Path::new("tests/fake_rust/kpop_graph/chain_b.rs"));
    let chain_c = parse_rs(Path::new("tests/fake_rust/kpop_graph/chain_c.rs"));
    let chain_d = parse_rs(Path::new("tests/fake_rust/kpop_graph/chain_d.rs"));
    let parsed: Vec<&ParsedRustFile> = vec![&chain_a, &chain_b, &chain_c, &chain_d];
    let graph = build_rust_dependency_graph(&parsed);
    let metrics = graph.module_metrics("fake_rust.kpop_graph.chain_a");
    assert!(metrics.dependency_depth >= 3);
}

#[test]
fn kpop_rust_none_test_coverage_threshold() {
    // RULE: test_coverage_threshold (Rust)
    // Static-reference coverage was removed; runtime coverage is owned by `kiss test`.
    let gate = kiss::GateConfig {
        test_coverage_threshold: 90,
        ..Default::default()
    };
    assert_eq!(gate.test_coverage_threshold, 90);
    assert!(
        !std::fs::read_to_string("src/lib.rs")
            .unwrap()
            .contains("analyze_rust_test_refs"),
        "static-reference analyze_rust_test_refs must not be re-exported"
    );
}

#[test]
fn kpop_rust_none_min_similarity() {
    // RULE: min_similarity (Rust)
    // Use existing fake_rust duplicates.
    let a = parse_rs(Path::new("tests/fake_rust/duplicate1.rs"));
    let b = parse_rs(Path::new("tests/fake_rust/duplicate2.rs"));
    let parsed: Vec<&ParsedRustFile> = vec![&a, &b];
    let clusters = kiss::cluster_duplicates_from_chunks(
        &kiss::extract_rust_chunks_for_duplication(&parsed),
        &kiss::DuplicationConfig::default(),
    );
    assert!(!clusters.is_empty());
}
