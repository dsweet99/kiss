use super::*;

fn empty_cache(fp: &str) -> FullCheckCache {
    FullCheckCache {
        fingerprint: fp.to_string(),
        py_file_count: 0,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        graph_nodes: 0,
        graph_edges: 0,
        base_violations: Vec::new(),
        graph_violations: Vec::new(),
        coverage_violations: Vec::new(),
        py_duplicates: Vec::new(),
        rs_duplicates: Vec::new(),
        definitions: Vec::new(),
        unreferenced: Vec::new(),
    }
}

fn empty_inputs(fp: &str) -> FullCacheInputs<'static> {
    FullCacheInputs {
        fingerprint: fp.to_string(),
        py_file_count: 0,
        rs_file_count: 0,
        code_unit_count: 0,
        statement_count: 0,
        violations: &[],
        graph_viols_all: &[],
        coverage_violations: &[],
        py_graph: None,
        rs_graph: None,
        py_dups_all: &[],
        rs_dups_all: &[],
        definitions: Vec::new(),
        unreferenced: Vec::new(),
    }
}

#[test]
fn fingerprint_path_duplicates_and_coverage_helpers() {
    let fp = fingerprint_for_check(
        &[],
        &[],
        &Config::python_defaults(),
        &Config::rust_defaults(),
        &GateConfig::default(),
    );
    assert!(!fp.is_empty());

    let v = coverage_violation(PathBuf::from("test.py"), "foo".into(), 1, 50);
    assert_eq!(v.metric, "test_coverage");
    assert!(v.message.contains("50%"));
    assert_eq!(graph_counts(None, None), (0, 0));

    cache_path_full("deadbeef");
    assert!(load_full_cache("deadbeef").is_none());

    let focus = HashSet::new();
    let (_viols, py_dups, rs_dups, cache) =
        cached_duplicates(empty_cache("deadbeef"), &GateConfig::default(), &focus);
    assert!(py_dups.is_empty() && rs_dups.is_empty());
    assert!(cached_coverage_viols(&cache, &focus).is_empty());
}

#[test]
fn fnv1a64_properties() {
    let h0 = 0xcbf2_9ce4_8422_2325_u64;
    assert_eq!(fnv1a64(h0, b""), h0);
    assert_eq!(fnv1a64(h0, b"hello"), fnv1a64(h0, b"hello"));
    assert_ne!(fnv1a64(h0, b"hello"), fnv1a64(h0, b"world"));
}

#[test]
fn full_cache_inputs_and_store() {
    let mut inputs = empty_inputs("test_fp_persist");
    inputs.py_file_count = 1;
    assert_eq!(inputs.py_file_count, 1);
    store_full_cache_from_run(inputs);
    let loaded = load_full_cache("test_fp_persist");
    assert_eq!(loaded.as_ref().map(|c| c.fingerprint.as_str()), Some("test_fp_persist"));
    assert_eq!(loaded.map(|c| c.py_file_count), Some(1));
}

#[test]
fn fingerprint_includes_python_annotations_per_function() {
    let gate = GateConfig::default();
    let rs = Config::rust_defaults();
    let base = Config::python_defaults();
    let mut other = base.clone();
    other.annotations_per_function = base.annotations_per_function.saturating_add(1);
    assert_ne!(
        fingerprint_for_check(&[], &[], &base, &rs, &gate),
        fingerprint_for_check(&[], &[], &other, &rs, &gate),
    );
}

#[test]
fn fingerprint_includes_python_returns_per_function() {
    let gate = GateConfig::default();
    let rs = Config::rust_defaults();
    let base = Config::python_defaults();
    let mut other = base.clone();
    other.returns_per_function = base.returns_per_function.saturating_add(1);
    assert_ne!(
        fingerprint_for_check(&[], &[], &base, &rs, &gate),
        fingerprint_for_check(&[], &[], &other, &rs, &gate),
    );
}

#[test]
fn fingerprint_includes_gate_test_coverage_threshold() {
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let g0 = GateConfig::default();
    let mut g1 = g0.clone();
    g1.test_coverage_threshold = g0.test_coverage_threshold.saturating_add(1);
    assert_ne!(
        fingerprint_for_check(&[], &[], &py, &rs, &g0),
        fingerprint_for_check(&[], &[], &py, &rs, &g1),
    );
}
