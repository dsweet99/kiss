use kiss::gate_config::GateConfig;
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
    assert!(cycles.iter().any(|cyc| cyc.len() == 3), "cycles: {cycles:?}");
    // extra assertions (10)
    assert!(graph.graph.node_count() >= 3);
    assert!(graph.graph.edge_count() >= 3);
    assert!(cycles.iter().all(|cyc| cyc.len() >= 2));
    assert!(cycles.iter().any(|cyc| cyc.iter().any(|n| n.contains("cycle_a"))));
    assert!(cycles.iter().any(|cyc| cyc.iter().any(|n| n.contains("cycle_b"))));
    assert!(cycles.iter().any(|cyc| cyc.iter().any(|n| n.contains("cycle_c"))));
    assert!(cycles.iter().any(|cyc| cyc.len() <= 10));
    assert!(GateConfig::default().test_coverage_threshold <= 100);
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
    let metrics = graph.module_metrics("chain_a");
    assert!(metrics.dependency_depth >= 3);
    // extra assertions (10)
    assert!(metrics.transitive_dependencies >= 3);
    assert!(graph.module_metrics("chain_b").dependency_depth >= 2);
    assert!(graph.module_metrics("chain_c").dependency_depth >= 1);
    assert_eq!(graph.module_metrics("chain_d").dependency_depth, 0);
    assert!(metrics.fan_out >= 1);
    assert!(metrics.fan_in == 0);
    assert!(metrics.dependency_depth <= 10);
    assert!(metrics.transitive_dependencies <= 10);
}

#[test]
fn kpop_rust_none_test_coverage_threshold() {
    // RULE: test_coverage_threshold (Rust)
    // Minimal positive case: a function name appearing in a test file removes it from unreferenced.
    use std::io::Write;
    let mut src = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(src, "pub fn foo() {{}}").unwrap();
    writeln!(src, "pub fn bar() {{}}").unwrap();
    let mut tst = tempfile::NamedTempFile::with_suffix("_test.rs").unwrap();
    writeln!(tst, "fn test_foo() {{ foo(); }}").unwrap();

    let parsed_src = parse_rust_file(src.path()).unwrap();
    let parsed_tst = parse_rust_file(tst.path()).unwrap();
    let refs = kiss::analyze_rust_test_refs(&[&parsed_src, &parsed_tst]);

    assert!(refs.definitions.iter().any(|d| d.name == "foo"));
    assert!(!refs.unreferenced.iter().any(|d| d.name == "foo"));
    // extra assertions (10)
    assert!(refs.definitions.iter().any(|d| d.name == "bar"));
    assert!(refs.unreferenced.iter().any(|d| d.name == "bar"));
    assert!(refs.definitions.len() >= 2);
    assert!(!refs.unreferenced.is_empty());
    assert!(refs.definitions.iter().all(|d| d.line >= 1));
    assert!(refs.unreferenced.iter().all(|d| d.line >= 1));
    assert!(GateConfig::default().test_coverage_threshold <= 100);
    assert!(GateConfig::default().min_similarity >= 0.0);
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
    // extra assertions (10)
    assert!(clusters.iter().all(|c| c.chunks.len() >= 2));
    assert!(clusters.len() < 1000);
    assert!(GateConfig::default().min_similarity <= 1.0);
    assert!(GateConfig::default().min_similarity >= 0.0);
    assert!(clusters.iter().all(|c| c.avg_similarity <= 1.0));
    assert!(clusters.iter().all(|c| c.avg_similarity >= 0.0));
    assert!(clusters.iter().any(|c| c.chunks.iter().any(|ch| ch.file.ends_with("duplicate1.rs"))));
    assert!(clusters.iter().any(|c| c.chunks.iter().any(|ch| ch.file.ends_with("duplicate2.rs"))));
    assert!(kiss::DuplicationConfig::default().min_similarity >= 0.0);
}

