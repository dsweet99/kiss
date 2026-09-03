use super::{
    ResolveRustPopulationArgs, ResolvedRustPopulation, current_partial_population_covers_selection,
    planned_check_aggregate_line_selectors, resolve_rust_population_state,
    rust_coverage_cache_root, select_check_aggregate_source_selectors,
    select_rust_source_selectors_for_basis,
};
use crate::test_runner::coverage_decision::{CoverageFreshness, SelectionBasis};
use kiss::rust_llvm_cov_runner::RustPopulationState;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn witness_resolved_population_enum() {
    let resolved = ResolvedRustPopulation::ReusablePrior {
        state: RustPopulationState {
            input_fingerprint: String::new(),
            generation_fingerprint: String::new(),
            selection_context_fingerprint: String::new(),
            entries_fingerprint: String::new(),
            selectors: Vec::new(),
            line_index: BTreeMap::new(),
            ordinary_source_digests: BTreeMap::new(),
            test_binaries: BTreeMap::new(),
        },
        delta: kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
    };
    assert_eq!(resolved.basis(), SelectionBasis::ReusablePrior);
    assert_eq!(resolved.freshness(), CoverageFreshness::ReusablePrior);
    assert!(resolved.state().is_some());
    assert_eq!(
        ResolvedRustPopulation::StructuralStale.freshness(),
        CoverageFreshness::Stale
    );
    assert_eq!(
        ResolvedRustPopulation::ColdStale.basis(),
        SelectionBasis::Population
    );
    assert!(ResolvedRustPopulation::StructuralStale.state().is_none());
    let root = std::path::Path::new(".");
    let empty = BTreeMap::new();
    assert_eq!(
        select_rust_source_selectors_for_basis(
            root,
            &[],
            &empty,
            &[],
            &ResolvedRustPopulation::ColdStale,
        ),
        Some(BTreeSet::new())
    );
    assert!(
        select_rust_source_selectors_for_basis(
            root,
            &[std::path::PathBuf::from("src/lib.rs")],
            &empty,
            &[],
            &ResolvedRustPopulation::ColdStale,
        )
        .is_none()
    );
    assert!(!current_partial_population_covers_selection(
        root,
        &[],
        &empty,
        &[],
        resolved.state().unwrap(),
    ));
}

#[test]
fn partial_current_population_must_exactly_cover_changed_source_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "pub fn value() -> u32 { 1 }\n").unwrap();
    let population = RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["tests::covers_src".to_string()],
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::from(["tests::covers_src".to_string()]),
        )]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };

    assert!(current_partial_population_covers_selection(
        tmp.path(),
        std::slice::from_ref(&src),
        &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        &[],
        &population
    ));
    let mut extra_manifest_selector = population.clone();
    extra_manifest_selector
        .selectors
        .push("tests::not_selected".to_string());
    assert!(!current_partial_population_covers_selection(
        tmp.path(),
        &[src],
        &BTreeMap::new(),
        &[],
        &extra_manifest_selector
    ));
}

#[test]
fn check_aggregate_source_selection_returns_population_or_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "pub fn value() -> u32 { 1 }\n").unwrap();

    let covered = RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "check-aggregate:deadbeef".to_string(),
        selectors: vec!["tests::covers_src".to_string()],
        line_index: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::new())]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let resolved = ResolvedRustPopulation::Current {
        state: covered.clone(),
    };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&src),
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("selection");
    assert_eq!(selected, BTreeSet::from(["tests::covers_src".to_string()]));
    let new_src = tmp.path().join("src").join("new.rs");
    std::fs::write(&new_src, "pub fn new_value() -> u32 { 2 }\n").unwrap();
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            &[src.clone(), new_src],
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none(),
        "a mixed covered/uncovered aggregate selection must fail closed"
    );

    let uncovered = RustPopulationState {
        line_index: BTreeMap::new(),
        ..covered
    };
    let resolved = ResolvedRustPopulation::Current { state: uncovered };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        &[src],
        &BTreeMap::new(),
        &[],
        &resolved,
    );
    assert!(
        selected.is_none(),
        "an aggregate cannot prove completeness for an uncovered source"
    );
}

#[test]
fn check_aggregate_source_selection_uses_binary_file_attribution() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src/a.rs");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "pub fn a() {}\n").unwrap();
    let cache_root = rust_coverage_cache_root(tmp.path());
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let bin_a = target.join("bin-a");
    let bin_b = target.join("bin-b");
    std::fs::write(&bin_a, "binary a").unwrap();
    std::fs::write(&bin_b, "binary b").unwrap();
    let digest = |path: &std::path::Path| {
        std::fs::read(path)
            .unwrap()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            })
    };
    let req = kiss::rust_llvm_cov_runner::RustCoverageBatchRequest {
        cwd: tmp.path().to_path_buf(),
        source_root: tmp.path().to_path_buf(),
        cargo: "cargo".into(),
        cache_root: cache_root.clone(),
        logical_selectors: vec!["alpha".into(), "beta".into()],
        cargo_args: vec!["--workspace".into()],
        test_args: Vec::new(),
        env: BTreeMap::new(),
        force_rerun: false,
        force_rerun_selectors: Vec::new(),
        jobs: 1,
        generated_config: cache_root.join("nextest.toml"),
        population_publication_selectors: None,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: "runner".into(),
        host_platform: "x86_64-unknown-linux-gnu".into(),
        coverage_output_mode: kiss::rust_llvm_cov_runner::CoverageOutputMode::CheckAggregate {
            publication_binary_ids: None,
            repair_publication: None,
        },
        selector_timeout_millis: BTreeMap::new(),
        cache_policy: kiss::test_cache_policy::TestCachePolicy::default(),
    };
    let identity = kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: "generation".into(),
        selection_context_fingerprint: "selection".into(),
        ordinary_source_digests: BTreeMap::from([("src/a.rs".into(), "digest".into())]),
    };
    let binaries = vec![
        kiss::rust_llvm_cov_runner::RustTestBinaryIdentity {
            id: "bin-a".into(),
            executable: bin_a.to_string_lossy().into_owned(),
            digest: format!("{:016x}", digest(&bin_a)),
        },
        kiss::rust_llvm_cov_runner::RustTestBinaryIdentity {
            id: "bin-b".into(),
            executable: bin_b.to_string_lossy().into_owned(),
            digest: format!("{:016x}", digest(&bin_b)),
        },
    ];
    let aggregate = kiss::rust_llvm_cov_runner::build_check_aggregate(
        &req,
        &identity,
        &["alpha".into(), "beta".into()],
        BTreeMap::from([
            ("alpha".into(), vec!["bin-a".into()]),
            ("beta".into(), vec!["bin-b".into()]),
        ]),
        &binaries,
        BTreeMap::from([
            (
                "bin-a".into(),
                kiss::rust_llvm_cov_runner::RustLineCoverage {
                    files: BTreeMap::from([("src/a.rs".into(), BTreeSet::from([1]))]),
                },
            ),
            (
                "bin-b".into(),
                kiss::rust_llvm_cov_runner::RustLineCoverage {
                    files: BTreeMap::from([("src/b.rs".into(), BTreeSet::from([1]))]),
                },
            ),
        ]),
    )
    .unwrap();
    kiss::rust_llvm_cov_runner::publish_check_aggregate(&req, &aggregate).unwrap();
    let population = RustPopulationState {
        input_fingerprint: "input".into(),
        generation_fingerprint: "generation".into(),
        selection_context_fingerprint: "selection".into(),
        entries_fingerprint: "check-aggregate:fixture".into(),
        selectors: vec!["alpha".into(), "beta".into()],
        line_index: BTreeMap::from([
            ("src/a.rs".into(), BTreeSet::new()),
            ("src/b.rs".into(), BTreeSet::new()),
        ]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: binaries
            .iter()
            .cloned()
            .map(|binary| (binary.id.clone(), binary))
            .collect(),
    };
    assert_eq!(
        select_check_aggregate_source_selectors(tmp.path(), &[src], &population),
        Some(BTreeSet::from(["alpha".to_string()]))
    );
    std::fs::write(&bin_a, "changed binary a").unwrap();
    assert_eq!(
        select_check_aggregate_source_selectors(
            tmp.path(),
            &[tmp.path().join("src/a.rs")],
            &population,
        ),
        Some(BTreeSet::from(["alpha".to_string(), "beta".to_string()])),
        "stale binary attribution must widen to the full population"
    );
}

#[test]
fn check_aggregate_line_selectors_drop_names_outside_the_planned_population() {
    let planned = vec!["tests::covers_src".to_string()];
    let selected = BTreeSet::from([
        "build_check_aggregate_reports_missing_binary_identity_and_line_map".to_string(),
        "tests::covers_src".to_string(),
    ]);
    assert_eq!(
        planned_check_aggregate_line_selectors(&selected, &planned),
        BTreeSet::from(["tests::covers_src".to_string()])
    );
    assert!(
        planned_check_aggregate_line_selectors(
            &BTreeSet::from(["nextest_only_name".to_string()]),
            &planned
        )
        .is_empty()
    );
}

#[test]
fn check_aggregate_rejects_non_rust_paths_on_reusable_prior() {
    let tmp = tempfile::tempdir().unwrap();
    let py = tmp.path().join("mod.py");
    std::fs::write(&py, "def x():\n    return 1\n").unwrap();
    let population = RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "check-aggregate:deadbeef".to_string(),
        selectors: vec!["tests::covers_src".to_string()],
        line_index: BTreeMap::from([("mod.py".to_string(), BTreeSet::new())]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let resolved = ResolvedRustPopulation::ReusablePrior {
        state: population,
        delta: kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
    };
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            &[py],
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none()
    );
}

#[test]
fn resolve_exact_partial_and_reusable_loader_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    std::fs::write(
        &src,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn gets_value() { assert_eq!(super::value(), 1); }\n}\n",
    )
    .unwrap();
    crate::test_runner::rust_coverage_index::write_test_entry(
        tmp.path(),
        "value",
        "tests::gets_value",
        kiss::rpytest_runner::TestStatus::Passed,
        kiss::rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    crate::test_runner::rust_coverage_index::rebuild_rust_coverage_index(tmp.path()).unwrap();
    crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
        tmp.path(),
        &["tests::gets_value".to_string()],
        &[],
    )
    .unwrap();

    let exact_expected = ["tests::gets_value".to_string()];
    let exact = resolve_rust_population_state(ResolveRustPopulationArgs {
        repo_root: tmp.path(),
        ignore: &[],
        rust_source_paths: std::slice::from_ref(&src),
        rust_changed_lines: &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        expected_selectors: Some(&exact_expected),
        test_args: &[],
    })
    .expect("exact");
    assert!(matches!(exact, ResolvedRustPopulation::Current { .. }));

    let partial_expected = [
        "tests::gets_value".to_string(),
        "tests::new_selector".to_string(),
    ];
    let partial = resolve_rust_population_state(ResolveRustPopulationArgs {
        repo_root: tmp.path(),
        ignore: &[],
        rust_source_paths: std::slice::from_ref(&src),
        rust_changed_lines: &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        expected_selectors: Some(&partial_expected),
        test_args: &[],
    })
    .expect("partial");
    assert!(matches!(partial, ResolvedRustPopulation::Current { .. }));

    std::fs::write(
        &src,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn gets_value() { assert_eq!(super::value(), 2); }\n}\n",
    )
    .unwrap();
    let reusable = resolve_rust_population_state(ResolveRustPopulationArgs {
        repo_root: tmp.path(),
        ignore: &[],
        rust_source_paths: std::slice::from_ref(&src),
        rust_changed_lines: &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        expected_selectors: Some(&exact_expected),
        test_args: &[],
    })
    .expect("reusable");
    assert!(matches!(
        reusable,
        ResolvedRustPopulation::ReusablePrior { .. }
    ));
}
