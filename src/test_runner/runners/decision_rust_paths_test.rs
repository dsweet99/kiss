use super::*;

fn empty_population_state() -> kiss::rust_llvm_cov_runner::RustPopulationState {
    kiss::rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    }
}

fn resolved_current() -> ResolvedRustPopulation {
    ResolvedRustPopulation::Current {
        state: empty_population_state(),
    }
}

fn resolved_reusable(
    delta: kiss::rust_llvm_cov_runner::RustSnapshotDelta,
) -> ResolvedRustPopulation {
    ResolvedRustPopulation::ReusablePrior {
        state: empty_population_state(),
        delta,
    }
}

#[test]
fn prepare_rust_inputs_carries_no_resolution_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let py = tmp.path().join("app.py");
    let rs = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(rs.parent().unwrap()).unwrap();
    std::fs::write(&py, "VALUE = 1\n").unwrap();
    std::fs::write(&rs, "pub fn value() -> u32 { 1 }\n").unwrap();
    let prepared = prepare_rust_inputs(
        tmp.path(),
        &[py.clone(), rs.clone()],
        &[],
        &BTreeMap::from([(py.clone(), BTreeSet::from([1]))]),
        &[],
        Some(kiss::Language::Python),
        &[],
    )
    .unwrap();

    assert_eq!(prepared.py_source_paths, vec![py]);
    assert_eq!(prepared.rust_source_paths, vec![rs]);
    assert_eq!(prepared.rust_vcs_source_paths, 1);
    assert_eq!(prepared.rust_snapshot_delta_modified, 0);
    assert!(!prepared.rust_snapshot_delta_structural);
    assert!(prepared.rust_resolved.is_none());
    assert!(prepared.changed_tests.python.is_empty());
    assert!(prepared.changed_tests.rust.is_empty());
    assert!(prepared.rust_changed_lines.is_empty());
}

#[test]
fn effective_paths_follow_current_and_reusable_delta_rules() {
    let vcs_source = PathBuf::from("src/lib.rs");
    let vcs_test = PathBuf::from("tests/integration.rs");
    assert_eq!(
        effective_paths_for_resolution(
            &resolved_current(),
            std::slice::from_ref(&vcs_source),
            std::slice::from_ref(&vcs_test),
            Path::new("."),
            &[],
        )
        .unwrap(),
        (vec![vcs_source], vec![vcs_test], 0, false)
    );

    let modified = PathBuf::from("src/changed.rs");
    assert_eq!(
        effective_paths_for_resolution(
            &resolved_reusable(kiss::rust_llvm_cov_runner::RustSnapshotDelta::Modified(
                vec![modified.clone()]
            )),
            &[],
            &[],
            Path::new("."),
            &[],
        )
        .unwrap(),
        (vec![modified], Vec::new(), 1, false)
    );

    assert_eq!(
        effective_paths_for_resolution(
            &resolved_reusable(kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged),
            &[],
            &[],
            Path::new("."),
            &[],
        )
        .unwrap(),
        (Vec::new(), Vec::new(), 0, false)
    );
}

#[test]
fn effective_paths_mark_structural_population() {
    assert_eq!(
        effective_paths_for_resolution(
            &ResolvedRustPopulation::StructuralStale,
            &[],
            &[],
            Path::new("."),
            &[],
        )
        .unwrap(),
        (Vec::new(), Vec::new(), 0, true)
    );
    let vcs_source = PathBuf::from("src/lib.rs");
    assert_eq!(
        effective_paths_for_resolution(
            &ResolvedRustPopulation::ColdStale,
            std::slice::from_ref(&vcs_source),
            &[],
            Path::new("."),
            &[],
        )
        .unwrap(),
        (vec![vcs_source], Vec::new(), 0, false)
    );
}

#[test]
fn effective_rust_changed_lines_survive_line_aware_basis() {
    let path = PathBuf::from("src/lib.rs");
    let changed = BTreeMap::from([(path.clone(), BTreeSet::from([7]))]);
    assert_eq!(
        effective_rust_changed_lines(
            &changed,
            std::slice::from_ref(&path),
            Some(&resolved_current()),
        ),
        changed
    );
    assert_eq!(
        effective_rust_changed_lines(
            &changed,
            std::slice::from_ref(&path),
            Some(&resolved_reusable(
                kiss::rust_llvm_cov_runner::RustSnapshotDelta::Modified(vec![path.clone()]),
            )),
        ),
        changed
    );
    assert!(
        effective_rust_changed_lines(
            &changed,
            &[],
            Some(&resolved_reusable(
                kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
            )),
        )
        .is_empty()
    );
}

#[test]
fn prepared_and_effective_rust_input_unit_witnesses() {
    let prepared = PreparedRustInputs {
        py_source_paths: Vec::new(),
        rust_source_paths: Vec::new(),
        python_changed_lines: BTreeMap::new(),
        rust_changed_lines: BTreeMap::new(),
        changed_tests: ChangedTestSelectors::default(),
        rust_resolved: None,
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
    };
    let effective = EffectiveRustPaths {
        source_paths: Vec::new(),
        test_paths: Vec::new(),
        resolved: None,
        snapshot_delta_modified: 0,
        snapshot_delta_structural: false,
    };
    assert_eq!(
        prepared.rust_vcs_source_paths,
        effective.snapshot_delta_modified
    );
    assert!(effective.source_paths.is_empty() && effective.test_paths.is_empty());
    assert!(effective.resolved.is_none() && !effective.snapshot_delta_structural);
}
