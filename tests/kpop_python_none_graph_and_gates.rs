use kiss::gate_config::GateConfig;
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

    // extra assertions (10)
    assert!(graph.graph.node_count() >= 3);
    assert!(graph.graph.edge_count() >= 3);
    assert!(cycles.iter().all(|cyc| cyc.len() >= 2));
    assert!(cycles.iter().any(|cyc| cyc.iter().any(|n| n.contains("cycle_a"))));
    assert!(cycles.iter().any(|cyc| cyc.iter().any(|n| n.contains("cycle_b"))));
    assert!(cycles.iter().any(|cyc| cyc.iter().any(|n| n.contains("cycle_c"))));
    assert!(cycles.iter().any(|cyc| cyc.len() <= 10));
    assert!(any3);
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

    // extra assertions (10)
    assert!(metrics.transitive_dependencies >= 3);
    let mb = graph.module_metrics("tests.fake_python.kpop_graph.chain_b");
    assert!(mb.dependency_depth >= 2);
    let mc = graph.module_metrics("tests.fake_python.kpop_graph.chain_c");
    assert!(mc.dependency_depth >= 1);
    let md = graph.module_metrics("tests.fake_python.kpop_graph.chain_d");
    assert_eq!(md.dependency_depth, 0);
    assert!(metrics.dependency_depth >= mb.dependency_depth);
    assert!(mb.dependency_depth >= mc.dependency_depth);
    assert!(mc.dependency_depth >= md.dependency_depth);
    assert!(metrics.fan_out >= 1);
}

#[test]
fn kpop_python_none_test_coverage_threshold() {
    // RULE: test_coverage_threshold
    //
    // KPOP hypothesis: test-ref analysis considers a definition "covered" if its name appears in a test file.
    // We test a small positive case.
    let code = {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(
            tmp,
            "def foo():\n    return 1\n\ndef bar():\n    return 2\n"
        )
        .unwrap();
        tmp
    };
    let test_code = {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::with_suffix("_test.py").unwrap();
        write!(tmp, "from x import foo\n\ndef test_foo():\n    foo()\n").unwrap();
        tmp
    };
    let mut parser = create_parser().unwrap();
    let parsed_code = parse_file(&mut parser, code.path()).unwrap();
    let parsed_test = parse_file(&mut parser, test_code.path()).unwrap();
    let refs = kiss::analyze_test_refs(&[&parsed_code, &parsed_test]);

    // We expect at least one definition (foo) to not be unreferenced.
    assert!(refs.definitions.iter().any(|d| d.name == "foo"));
    assert!(!refs.unreferenced.iter().any(|d| d.name == "foo"));

    // extra assertions (10)
    assert!(refs.definitions.iter().any(|d| d.name == "bar"));
    assert!(refs.unreferenced.iter().any(|d| d.name == "bar"));
    assert!(GateConfig::default().test_coverage_threshold <= 100);
    assert!(refs.definitions.len() >= 2);
    assert!(!refs.unreferenced.is_empty());
    assert!(refs.definitions.iter().all(|d| d.line >= 1));
    assert!(refs.unreferenced.iter().all(|d| d.line >= 1));
    assert!(refs.definitions.iter().any(|d| d.file.extension().is_some()));
    assert!(refs.unreferenced.iter().any(|d| d.file.extension().is_some()));
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

    // extra assertions (10)
    assert!(dups[0].similarity >= 0.7);
    assert!(GateConfig::default().min_similarity >= 0.0);
    assert!(GateConfig::default().min_similarity <= 1.0);
    assert!(dups.iter().all(|d| d.similarity <= 1.0));
    assert!(dups.iter().all(|d| d.similarity >= 0.0));
    assert!(dups.iter().any(|d| d.chunk1.file.ends_with("user_service.py")));
    assert!(dups.iter().any(|d| d.chunk2.file.ends_with("user_service.py")));
    assert!(dups.len() < 1000);
    assert!(kiss::DuplicationConfig::default().min_similarity >= 0.0);
}

