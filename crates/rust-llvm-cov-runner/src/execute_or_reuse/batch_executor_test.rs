use super::*;
use crate::RustCovCacheStatus;
use crate::RustLineCoverage;
use crate::RustLlvmCovError;
use crate::execute_or_reuse::batch_lock::lock_batch;
use crate::publish_derived_state;
use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_alpha_entry,
    store_batch_executor_selector, witness_batch_tools,
};
use rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::mpsc;
use std::time::Duration;

fn tools() -> crate::RustCoverageToolIdentity {
    witness_batch_tools()
}

#[test]
fn all_hit_batch_returns_without_batch_lock_or_spawn() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");

    let result = execute_rust_coverage_batch(&req, &tools()).unwrap();
    assert_eq!(result.completed.len(), 2);
    assert!(result.batch_error.is_none());
    assert_eq!(result.counters.cache_hits, 2);
    assert_eq!(result.counters.build_invocations, 0);
    assert!(
        result
            .completed
            .iter()
            .all(|outcome| { outcome.cache_status == RustCovCacheStatus::Hit })
    );
}

#[test]
fn all_hit_derived_repair_acquires_batch_lock_without_spawn() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);

    let result = execute_rust_coverage_batch(&population_req, &tools()).unwrap();
    assert_eq!(result.counters.cache_hits, 2);
    assert_eq!(result.counters.build_invocations, 0);
    assert!(result.counters.derived_state_published);
    assert!(result.counters.derived_repair);
}

#[test]
fn all_hit_derived_repair_reports_deferred_legacy_cleanup() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);
    fs::create_dir_all(population_req.cache_root.join("workers").join("slot-0")).unwrap();
    let _slot_guard = crate::execute_or_reuse::worker::lock_worker_for_test(&population_req.cache_root, 0).unwrap();

    let result = execute_rust_coverage_batch(&population_req, &tools()).unwrap();

    assert_eq!(result.counters.cache_hits, 2);
    assert!(result.counters.legacy_cleanup_deferred);
}

#[test]
fn post_lock_recheck_becomes_hit_after_another_process_stores_entry() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let mut alpha_req = req.clone();
    alpha_req.logical_selectors = vec!["alpha".to_string()];
    let cache_root = req.cache_root.clone();
    let publisher_req = req.clone();

    let (lock_acquired_tx, lock_acquired_rx) = mpsc::channel();
    let publisher = std::thread::spawn(move || {
        let _guard = lock_batch(&cache_root).unwrap();
        lock_acquired_tx.send(()).expect("publisher lock signal");
        std::thread::sleep(Duration::from_millis(100));
        store_batch_executor_selector(publisher_req.cwd.as_path(), &publisher_req, "alpha");
    });

    lock_acquired_rx
        .recv()
        .expect("publisher must acquire batch lock before recheck");
    let result = execute_rust_coverage_batch(&alpha_req, &tools()).unwrap();
    publisher.join().unwrap();

    assert_eq!(result.completed.len(), 1);
    assert_eq!(result.completed[0].cache_status, RustCovCacheStatus::Hit);
    assert_eq!(result.counters.cache_hits, 1);
    assert_eq!(result.counters.build_invocations, 0);
}

#[test]
fn failed_selective_fresh_batch_preserves_population_through_executor() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["alpha".to_string()];
    req.population_publication_selectors = Some(vec!["alpha".to_string()]);
    let tools = tools();
    let population_identity = batch_identity(&req, &tools).unwrap();
    store_alpha_entry(
        &req.cache_root,
        &req,
        &tools,
        &population_identity,
        BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
    );
    publish_derived_state(
        &req,
        &tools,
        &population_identity,
        &["alpha".to_string()],
        false,
    )
    .unwrap();
    let before_population = fs::read(req.cache_root.join("population.json")).unwrap();
    let before_index = fs::read(req.cache_root.join("index.json")).unwrap();
    store_obsolete_selective_entry(&req.cache_root);
    fs::write(repo.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();

    let mut selective = req.clone();
    selective.population_publication_selectors = None;
    let result = execute_rust_coverage_batch_with_fresh(
        &selective,
        &tools,
        |_req, _tools, _identity, _plan| {
            Ok(RustCoverageBatchResult {
                completed: Vec::new(),
                counters: RustCoverageBatchCounters::default(),
                batch_error: Some(RustLlvmCovError::InvalidRequest(
                    "injected selective failure through executor".to_string(),
                )),
                test_binaries: Vec::new(),
            })
        },
    )
    .unwrap();

    assert!(result.batch_error.is_some());
    assert_eq!(result.counters.cache_pruned_entries, 0);
    assert!(!result.counters.derived_state_published);
    assert_eq!(
        fs::read(selective.cache_root.join("population.json")).unwrap(),
        before_population
    );
    assert_eq!(
        fs::read(selective.cache_root.join("index.json")).unwrap(),
        before_index
    );
    assert!(
        selective
            .cache_root
            .join("entries")
            .join("obsoleteselective01.json")
            .is_file()
    );
    let manifest = crate::publish_derived::batch_derived_index::read_population_manifest(&selective.cache_root)
        .expect("population manifest");
    assert_eq!(
        manifest.generation_fingerprint,
        population_identity.generation_fingerprint
    );
}

#[test]
fn finalize_after_fresh_batch_marks_legacy_cleanup_for_check_aggregate_mode() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let mut result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };

    finalize_after_fresh_batch(&req, &tools, &identity, true, &mut result).unwrap();

    assert!(result.counters.legacy_cleanup_deferred);
}

#[test]
fn check_aggregate_population_rechecks_cache_after_lock_without_fresh_run() {
    let repo = batch_executor_fixture_repo();
    fs::create_dir_all(repo.path().join("target")).unwrap();
    let binary_path = repo.path().join("target").join("bin-a");
    fs::write(&binary_path, "binary-a").unwrap();
    let mut req = batch_executor_request(repo.path());
    req.force_rerun = true;
    req.population_publication_selectors = Some(req.logical_selectors.clone());
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let binary = crate::RustTestBinaryIdentity {
        id: "bin-a".to_string(),
        executable: binary_path.to_string_lossy().to_string(),
        digest: "aaaaaaaaaaaaaaaa".to_string(),
    };
    let aggregate = crate::build_check_aggregate(
        &req,
        &identity,
        &req.logical_selectors,
        BTreeMap::from([
            ("alpha".to_string(), vec!["bin-a".to_string()]),
            ("beta".to_string(), vec!["bin-a".to_string()]),
        ]),
        std::slice::from_ref(&binary),
        BTreeMap::from([(
            "bin-a".to_string(),
            RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
            },
        )]),
    )
    .unwrap();
    crate::publish_check_aggregate(&req, &aggregate).unwrap();
    crate::publish_derived::batch_derived::publish_conservative_derived_state_from_check_aggregate(
        &req, &tools, &identity, &aggregate,
    )
    .unwrap();

    let result =
        execute_rust_coverage_batch_with_fresh(&req, &tools, |_req, _tools, _identity, _plan| {
            panic!("fresh check-aggregate run should be skipped after lock recheck")
        })
        .unwrap();

    assert_eq!(result.completed.len(), 2);
    assert_eq!(result.counters.cache_hits, 2);
    assert_eq!(result.counters.build_invocations, 0);
    assert!(
        result
            .completed
            .iter()
            .all(|outcome| outcome.cache_status == RustCovCacheStatus::Hit)
    );
}

#[test]
fn apply_population_derived_publication_skips_errors_and_missing_selectors() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let mut errored = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: Some(RustLlvmCovError::InvalidRequest("failed".to_string())),
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };
    apply_population_derived_publication(&req, &tools, &identity, &mut errored).unwrap();
    assert!(!errored.counters.derived_state_published);

    let mut no_population = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };
    apply_population_derived_publication(&req, &tools, &identity, &mut no_population).unwrap();
    assert!(!no_population.counters.derived_state_published);
}

#[test]
fn outcome_from_entry_replays_cache_entry_without_output() {
    let entry = RustCovCacheEntry::from_outcome(
        &crate::RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
            },
            test_binary_ids: vec!["bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: Some(b"stdout".to_vec()),
            stderr: Some(b"stderr".to_vec()),
        },
        "generation",
    );

    let outcome = outcome_from_entry(entry, RustCovCacheStatus::Hit);

    assert_eq!(outcome.selector, "alpha");
    assert_eq!(outcome.cache_status, RustCovCacheStatus::Hit);
    assert_eq!(outcome.stdout, None);
    assert_eq!(outcome.stderr, None);
}

fn store_obsolete_selective_entry(cache_root: &std::path::Path) {
    let obsolete = RustCovCacheEntry::from_outcome(
        &crate::RustLlvmCovOutcome {
            selector: "obsolete".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        "obsolete-selective-generation",
    );
    store_rust_cov_cache_entry(cache_root, "obsoleteselective01", &obsolete).unwrap();
}
