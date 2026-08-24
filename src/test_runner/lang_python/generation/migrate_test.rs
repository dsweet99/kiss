use super::{
    V1Bundle, evidence_from_v1_bundle, selector_coverage_from_index,
    try_migrate_complete_v1_generation,
};
use crate::test_runner::python_coverage_index::{
    python_coverage_cache_root, read_python_population_manifest, write_population_durations,
    write_python_coverage_index_with_entries_fingerprint, write_python_coverage_snapshot,
    write_python_population_manifest_for_args,
};
use crate::test_runner::runners::detect_rslip_versions;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::Duration;

#[test]
fn migrate_returns_none_when_v1_bundle_incomplete() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let migrated = try_migrate_complete_v1_generation(repo, &[], &|_, _| true).unwrap();
    assert!(migrated.is_none());
}

#[test]
fn selector_coverage_from_index_maps_forward_index() {
    let index = BTreeMap::from([(
        "app.py".to_string(),
        BTreeSet::from(["t.py::a".to_string(), "t.py::b".to_string()]),
    )]);
    let coverage = BTreeMap::from([("app.py".to_string(), BTreeSet::from([1u32, 2]))]);
    let cov = selector_coverage_from_index("t.py::a", &index, &coverage);
    assert_eq!(cov.get("app.py"), Some(&BTreeSet::from([1u32, 2])));
    assert!(selector_coverage_from_index("missing", &index, &coverage).is_empty());
}

#[test]
fn evidence_from_v1_bundle_builds_complete_evidence() {
    let selectors = vec!["t.py::a".into()];
    let bundle = V1Bundle {
        selectors: selectors.clone(),
        coverage: BTreeMap::from([("app.py".into(), BTreeSet::from([1u32]))]),
        index: BTreeMap::from([("app.py".into(), BTreeSet::from(["t.py::a".into()]))]),
        durations: BTreeMap::from([("t.py::a".into(), Duration::from_millis(2))]),
    };
    let evidence = evidence_from_v1_bundle(&selectors, &bundle).expect("complete");
    assert!(evidence.complete);

    let with_empty = evidence_from_v1_bundle(
        &selectors,
        &V1Bundle {
            selectors: selectors.clone(),
            coverage: BTreeMap::new(),
            index: BTreeMap::new(),
            durations: BTreeMap::from([("t.py::a".into(), Duration::from_millis(1))]),
        },
    )
    .expect("empty coverage still migrates");
    assert!(with_empty.complete);
}

#[test]
fn migrate_publishes_generation_from_complete_v1_bundle() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::env::set_current_dir(repo).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let selector = "t.py::a".to_string();
    write_python_population_manifest_for_args(repo, std::slice::from_ref(&selector), &[]).unwrap();
    let manifest = read_python_population_manifest(repo).unwrap();
    let cache_root = python_coverage_cache_root(repo).unwrap();
    let coverage = BTreeMap::from([("app.py".into(), BTreeSet::from([1u32]))]);
    write_python_coverage_snapshot(repo, &coverage).unwrap();
    let index = BTreeMap::from([("app.py".into(), BTreeSet::from([selector.clone()]))]);
    write_python_coverage_index_with_entries_fingerprint(
        repo,
        &index,
        &manifest.entries_fingerprint,
    )
    .unwrap();
    write_population_durations(
        &cache_root,
        &manifest,
        &[(selector.clone(), Duration::from_millis(4))],
    )
    .unwrap();

    let migrated = try_migrate_complete_v1_generation(repo, &[], &|_, _| true)
        .unwrap()
        .expect("v1 migrate");
    assert!(!migrated.is_empty());
    assert!(
        cache_root.join("generations").join(&migrated).is_dir(),
        "generation dir must exist"
    );
}
