use super::{
    invalidate_population_durations, load_current_population_durations,
    load_durations_from_entries, population_durations_path, population_entries_all_pass,
    population_nonpassed_selectors, try_load_population_durations,
    try_publish_durations_after_population, write_population_durations,
};
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity;
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::RustPopulationState;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, published_alpha_derived_fixture,
    store_batch_executor_selector, witness_batch_tools,
};
use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

fn population(selectors: &[&str]) -> RustPopulationState {
    RustPopulationState {
        input_fingerprint: "in1".into(),
        generation_fingerprint: "gen1".into(),
        selection_context_fingerprint: "sel1".into(),
        entries_fingerprint: "ent1".into(),
        selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    }
}

#[test]
fn population_durations_round_trip_and_invalidate() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a", "b"]);
    let pairs = vec![
        ("a".into(), Duration::from_millis(12)),
        ("b".into(), Duration::from_millis(34)),
    ];
    write_population_durations(tmp.path(), &pop, &pairs).unwrap();
    assert!(population_durations_path(tmp.path()).is_file());
    let loaded = try_load_population_durations(tmp.path(), &pop).expect("hit");
    assert_eq!(loaded, pairs);

    let mut mismatched = pop.clone();
    mismatched.entries_fingerprint = "other".into();
    assert!(try_load_population_durations(tmp.path(), &mismatched).is_none());

    invalidate_population_durations(tmp.path());
    assert!(try_load_population_durations(tmp.path(), &pop).is_none());
}

#[test]
fn valid_population_durations_avoid_entry_directory_dependency() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a", "b"]);
    write_population_durations(
        tmp.path(),
        &pop,
        &[
            ("a".into(), Duration::from_millis(1)),
            ("b".into(), Duration::from_millis(2)),
        ],
    )
    .unwrap();
    assert!(population_entries_all_pass(tmp.path(), &pop));
    assert!(population_nonpassed_selectors(tmp.path(), &pop).is_empty());
    assert!(!tmp.path().join("entries").exists());
}

#[test]
fn population_certificate_rejects_entry_directory_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("entries")).unwrap();
    let pop = population(&["a"]);
    let pairs = vec![("a".into(), Duration::from_millis(1))];
    write_population_durations(tmp.path(), &pop, &pairs).unwrap();
    assert!(try_load_population_durations(tmp.path(), &pop).is_some());
    fs::write(tmp.path().join("entries/a.json"), b"changed entry").unwrap();
    assert!(try_load_population_durations(tmp.path(), &pop).is_none());
}

#[test]
fn population_durations_rejects_incomplete_map() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a", "b"]);
    let pairs = vec![("a".into(), Duration::from_millis(1))];
    assert!(write_population_durations(tmp.path(), &pop, &pairs).is_err());
}

#[test]
fn population_durations_rejects_duplicate_selectors_collapsing_count() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a", "b"]);
    let pairs = vec![
        ("a".into(), Duration::from_millis(1)),
        ("a".into(), Duration::from_millis(2)),
    ];
    assert!(write_population_durations(tmp.path(), &pop, &pairs).is_err());
}

#[test]
fn population_durations_rejects_wrong_schema_file() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a"]);
    fs::write(
        population_durations_path(tmp.path()),
        r#"{"schema_version":"nope","cache_schema_version":"x","generation_fingerprint":"gen1","input_fingerprint":"in1","entries_fingerprint":"ent1","durations":{"a":1}}"#,
    )
    .unwrap();
    assert!(try_load_population_durations(tmp.path(), &pop).is_none());
}

#[test]
fn load_and_publish_durations_from_entries_round_trip() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");

    let pop = RustPopulationState {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        entries_fingerprint: "ent-fixture".into(),
        selectors: vec!["alpha".into(), "beta".into()],
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let pairs = load_durations_from_entries(&req.cache_root, &pop, &identity, &req, &tools)
        .expect("entry durations");
    assert_eq!(pairs.len(), 2);
    assert!(pairs.iter().all(|(_, d)| *d == Duration::from_millis(7)));

    crate::rust_llvm_cov_runner::publish_next_entry_state(
        &req.cache_root,
        &identity.generation_fingerprint,
        "ent-fixture",
    )
    .unwrap();
    try_publish_durations_after_population(
        &req.cache_root,
        &identity,
        &req,
        &tools,
        &["alpha".into(), "beta".into()],
        "ent-fixture",
    );
    let loaded = try_load_population_durations(&req.cache_root, &pop).expect("sidecar hit");
    assert_eq!(loaded, pairs);
}

#[test]
fn load_durations_from_entries_fails_when_entry_missing() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let pop = RustPopulationState {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        entries_fingerprint: "ent-fixture".into(),
        selectors: vec!["alpha".into()],
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    assert!(load_durations_from_entries(&req.cache_root, &pop, &identity, &req, &tools).is_none());
    try_publish_durations_after_population(
        &req.cache_root,
        &identity,
        &req,
        &tools,
        &["alpha".into()],
        "ent-fixture",
    );
    assert!(try_load_population_durations(&req.cache_root, &pop).is_none());
}

#[test]
fn load_durations_from_entries_fails_on_generation_mismatch() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    let pop = RustPopulationState {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: "other-generation".into(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        entries_fingerprint: "ent-fixture".into(),
        selectors: vec!["alpha".into()],
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    assert!(load_durations_from_entries(&req.cache_root, &pop, &identity, &req, &tools).is_none());
}

#[test]
fn population_durations_round_trip_max_duration_nanos() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a"]);
    let pairs = vec![("a".into(), Duration::MAX)];
    write_population_durations(tmp.path(), &pop, &pairs).unwrap();
    let loaded = try_load_population_durations(tmp.path(), &pop).expect("hit");
    assert_eq!(loaded[0].1, Duration::from_nanos(u64::MAX));
}

#[test]
fn try_load_rejects_wrong_cache_schema_version() {
    let tmp = tempfile::tempdir().unwrap();
    let pop = population(&["a"]);
    write_population_durations(tmp.path(), &pop, &[("a".into(), Duration::from_millis(1))])
        .unwrap();
    let path = population_durations_path(tmp.path());
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["cache_schema_version"] = serde_json::Value::String("wrong-schema".into());
    fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
    assert!(try_load_population_durations(tmp.path(), &pop).is_none());
}

#[test]
fn load_current_population_durations_hits_manifest_sidecar_without_index() {
    let fixture = published_alpha_derived_fixture();

    let _ = load_current_population_durations(
        &fixture.req.cache_root,
        &fixture.req.source_root,
        &fixture.identity,
        &fixture.req,
        &fixture.tools,
        None,
    )
    .expect("seed sidecar");

    let _ = fs::remove_file(fixture.req.cache_root.join("index.json"));
    let hit = load_current_population_durations(
        &fixture.req.cache_root,
        &fixture.req.source_root,
        &fixture.identity,
        &fixture.req,
        &fixture.tools,
        None,
    )
    .expect("manifest+sidecar warm path");
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].0, "alpha");
}

#[test]
fn load_current_population_durations_hits_sidecar_then_rebuilds_on_miss() {
    let fixture = published_alpha_derived_fixture();
    let hit = load_current_population_durations(
        &fixture.req.cache_root,
        &fixture.req.source_root,
        &fixture.identity,
        &fixture.req,
        &fixture.tools,
        None,
    )
    .expect("sidecar or entry durations");
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].0, "alpha");
    assert_eq!(hit[0].1, Duration::from_millis(1));
    assert!(population_durations_path(&fixture.req.cache_root).is_file());

    invalidate_population_durations(&fixture.req.cache_root);
    assert!(!population_durations_path(&fixture.req.cache_root).is_file());
    let rebuilt = load_current_population_durations(
        &fixture.req.cache_root,
        &fixture.req.source_root,
        &fixture.identity,
        &fixture.req,
        &fixture.tools,
        None,
    )
    .expect("rebuild from entries");
    assert_eq!(rebuilt, hit);
    assert!(population_durations_path(&fixture.req.cache_root).is_file());
    let fingerprint = crate::rust_llvm_cov_runner::plan::batch_fingerprint::entry_fingerprint(
        &fixture.identity.input_digest,
        &fixture.req,
        &fixture.tools,
        "alpha",
    );
    let mut failed = crate::rust_llvm_cov_runner::rust_cov_cache::load_rust_cov_cache_entry(
        &fixture.req.cache_root,
        &fingerprint,
    )
    .unwrap();
    failed.status = crate::rpytest_runner::TestStatus::Failed;
    crate::rust_llvm_cov_runner::rust_cov_cache::store_rust_cov_cache_entry(
        &fixture.req.cache_root,
        &fingerprint,
        &failed,
    )
    .unwrap();
    assert!(
        load_current_population_durations(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            &fixture.req,
            &fixture.tools,
            None,
        )
        .is_none()
    );
}

#[test]
fn load_current_population_durations_returns_none_without_population() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    assert!(
        load_current_population_durations(
            &req.cache_root,
            &req.source_root,
            &identity,
            &req,
            &tools,
            None,
        )
        .is_none()
    );
}

#[test]
fn load_durations_from_entries_rejects_selector_mismatch() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    store_batch_executor_selector(repo.path(), &req, "alpha");
    let alpha_fp = crate::rust_llvm_cov_runner::plan::batch_fingerprint::entry_fingerprint(
        &identity.input_digest,
        &req,
        &tools,
        "alpha",
    );
    let beta_fp = crate::rust_llvm_cov_runner::plan::batch_fingerprint::entry_fingerprint(
        &identity.input_digest,
        &req,
        &tools,
        "beta",
    );
    let entry = crate::rust_llvm_cov_runner::rust_cov_cache::load_rust_cov_cache_entry(
        &req.cache_root,
        &alpha_fp,
    )
    .unwrap();

    crate::rust_llvm_cov_runner::rust_cov_cache::store_rust_cov_cache_entry(
        &req.cache_root,
        &beta_fp,
        &entry,
    )
    .unwrap();
    let pop = RustPopulationState {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        entries_fingerprint: "ent-fixture".into(),
        selectors: vec!["beta".into()],
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    assert!(load_durations_from_entries(&req.cache_root, &pop, &identity, &req, &tools).is_none());
}
