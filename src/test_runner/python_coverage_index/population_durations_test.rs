use super::*;
use crate::test_runner::python_coverage_index::write_python_population_manifest_for_args;
use std::time::Duration;

#[test]
fn python_population_durations_round_trip_via_sidecar() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    write_python_population_manifest_for_args(tmp.path(), std::slice::from_ref(&selector), &[])
        .unwrap();
    let manifest = read_python_population_manifest(tmp.path()).unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();
    let pairs = vec![(selector.clone(), Duration::from_millis(42))];
    write_population_durations(&cache_root, &manifest, &pairs).unwrap();
    let loaded = try_load_population_durations(&cache_root, &manifest).expect("sidecar hit");
    assert_eq!(loaded, pairs);
}

#[test]
fn write_population_durations_rejects_count_and_order_mismatch() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selectors = vec!["a".into(), "b".into()];
    write_python_population_manifest_for_args(tmp.path(), &selectors, &[]).unwrap();
    let manifest = read_python_population_manifest(tmp.path()).unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();
    assert!(
        write_population_durations(
            &cache_root,
            &manifest,
            &[("a".into(), Duration::from_millis(1))]
        )
        .is_err()
    );
    assert!(
        write_population_durations(
            &cache_root,
            &manifest,
            &[
                ("b".into(), Duration::from_millis(1)),
                ("a".into(), Duration::from_millis(2)),
            ]
        )
        .is_err()
    );
    write_population_durations(
        &cache_root,
        &manifest,
        &[
            ("a".into(), Duration::from_millis(1)),
            ("b".into(), Duration::from_millis(2)),
        ],
    )
    .unwrap();
    let loaded = try_load_population_durations(&cache_root, &manifest).unwrap();
    assert_eq!(loaded.len(), 2);
    assert!(try_publish_python_population_durations(tmp.path(), &[]).is_err());
}
