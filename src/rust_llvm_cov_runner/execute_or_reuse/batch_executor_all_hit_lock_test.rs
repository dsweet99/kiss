use super::*;
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::lock_batch;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity;
use crate::rust_llvm_cov_runner::RustCovCacheStatus;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
    witness_batch_tools,
};
use std::sync::mpsc;
use std::time::Duration;

fn tools() -> crate::rust_llvm_cov_runner::RustCoverageToolIdentity {
    witness_batch_tools()
}

#[test]
fn all_hit_fast_path_skips_publish_when_derived_already_valid() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);
    let tools = tools();
    let first = execute_rust_coverage_batch(&population_req, &tools).unwrap();
    assert!(first.counters.derived_state_published);
    let identity = batch_identity(&population_req, &tools).unwrap();
    let executable = repo.path().join("target/test-bin");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"test binary").unwrap();
    crate::rust_llvm_cov_runner::publish_derived_state_with_binaries(
        &population_req,
        &tools,
        &identity,
        &population_req.logical_selectors,
        &[crate::rust_llvm_cov_runner::RustTestBinaryIdentity {
            id: "test-bin".into(),
            executable: executable.to_string_lossy().to_string(),
            digest: crate::rust_llvm_cov_runner::rust_cov_cache::digest_test_binary(&executable)
                .unwrap(),
        }],
        false,
    )
    .unwrap();
    super::super::batch_warm_hit_seal::write_warm_all_hit_seal(&population_req, &identity).unwrap();
    assert_eq!(
        super::super::batch_executor_sealed::try_sealed_all_hit(
            &population_req,
            &identity,
            &tools,
        )
        .map(|result| result.counters.cache_hits),
        Some(2)
    );
    super::reset_lock_batch_call_count();
    let second = execute_rust_coverage_batch(&population_req, &tools).unwrap();
    assert_eq!(
        super::lock_batch_call_count(),
        0,
        "a sealed all-hit read must not acquire the writer lock"
    );
    assert!(
        second
            .completed
            .iter()
            .all(|outcome| !outcome.coverage.files.is_empty()),
        "sealed warm hits must preserve cached selector coverage"
    );
    assert_eq!(second.counters.cache_hits, 2);
    assert!(!second.counters.derived_state_published);
    assert!(!second.counters.derived_repair);
    assert_eq!(second.counters.build_invocations, 0);
    let third = execute_rust_coverage_batch(&population_req, &tools).unwrap();
    assert_eq!(third.counters.cache_hits, 2);
    assert_eq!(third.completed.len(), 2);
    assert!(!third.counters.derived_state_published);
    assert_eq!(third.counters.build_invocations, 0);
    let alpha = crate::rust_llvm_cov_runner::plan::batch_fingerprint::entry_fingerprint(
        &identity.input_digest,
        &population_req,
        &tools,
        "alpha",
    );
    std::fs::remove_file(
        crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_cache_entry_path(
            &population_req.cache_root,
            &alpha,
        ),
    )
    .unwrap();
    assert!(
        super::super::batch_executor_sealed::try_sealed_all_hit(
            &population_req,
            &identity,
            &tools,
        )
        .is_none()
    );
}

#[test]
fn changed_test_binary_rejects_seal_and_selector_hits() {
    let fixture = crate::rust_llvm_cov_runner::test_support::published_alpha_derived_fixture();
    let executable = fixture.repo.path().join("target/test-bin");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"original test binary").unwrap();
    let binary = crate::rust_llvm_cov_runner::RustTestBinaryIdentity {
        id: "test-bin".to_string(),
        executable: executable.to_string_lossy().to_string(),
        digest: crate::rust_llvm_cov_runner::rust_cov_cache::digest_test_binary(&executable)
            .unwrap(),
    };
    crate::rust_llvm_cov_runner::publish_derived_state_with_binaries(
        &fixture.req,
        &fixture.tools,
        &fixture.identity,
        &fixture.req.logical_selectors,
        &[binary],
        false,
    )
    .unwrap();
    crate::rust_llvm_cov_runner::invalidate_entry_state(&fixture.req.cache_root);
    let repaired = execute_rust_coverage_batch(&fixture.req, &fixture.tools).unwrap();
    assert!(repaired.counters.derived_repair);
    let repaired_manifest =
        crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::read_population_manifest(
            &fixture.req.cache_root,
        )
        .unwrap();
    assert!(
        !repaired_manifest.test_binaries.is_empty(),
        "identity-only all-hit repair must retain binary authority"
    );
    super::super::batch_warm_hit_seal::write_warm_all_hit_seal(&fixture.req, &fixture.identity)
        .unwrap();
    assert!(
        super::super::batch_executor_sealed::try_sealed_all_hit(
            &fixture.req,
            &fixture.identity,
            &fixture.tools,
        )
        .is_some()
    );
    std::fs::write(&executable, b"changed test binary").unwrap();
    assert!(
        super::super::batch_executor_sealed::try_sealed_all_hit(
            &fixture.req,
            &fixture.identity,
            &fixture.tools,
        )
        .is_none()
    );
    std::fs::write(fixture.req.cache_root.join("index.json"), b"broken index").unwrap();
    assert!(
        crate::rust_llvm_cov_runner::load_current_population_state(
            &fixture.req.cache_root,
            &fixture.req.source_root,
            &fixture.identity,
            None,
        )
        .is_none(),
        "the binary check below must use the manifest fallback"
    );
    let prepared = super::super::batch_executor_prepare::prepare_rust_batch(
        &fixture.req,
        &fixture.tools,
        &fixture.identity,
    )
    .unwrap();
    assert_eq!(prepared.misses, fixture.req.logical_selectors);
}

#[test]
fn batch_error_does_not_publish_warm_all_hit_seal() {
    let fixture = crate::rust_llvm_cov_runner::test_support::published_alpha_derived_fixture();
    let result = crate::rust_llvm_cov_runner::RustCoverageBatchResult {
        completed: vec![crate::rust_llvm_cov_runner::RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: crate::rust_llvm_cov_runner::RustLineCoverage::default(),
            test_binary_ids: Vec::new(),
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        }],
        batch_error: Some(RustLlvmCovError::InvalidRequest(
            "export failed".to_string(),
        )),
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };
    super::super::batch_executor_sealed::write_seal_after_complete_pass(
        &fixture.req,
        &fixture.identity,
        &result,
    );
    assert_eq!(
        super::super::batch_warm_hit_seal::try_warm_all_hit_seal(&fixture.req, &fixture.identity),
        None
    );
}

#[test]
fn all_hit_derived_repair_blocks_on_held_batch_lock() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);
    let cache_root = population_req.cache_root.clone();
    let (lock_held_tx, lock_held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = lock_batch(&cache_root).unwrap();
        lock_held_tx.send(()).expect("signal lock held");
        release_rx.recv().expect("wait for release");
    });
    lock_held_rx.recv().expect("holder must take batch.lock");
    let tools = tools();
    let repairer = std::thread::spawn(move || execute_rust_coverage_batch(&population_req, &tools));
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !repairer.is_finished(),
        "all-hit derived repair must block on batch.lock; finished early implies lock-free publish"
    );
    release_tx.send(()).expect("release lock");
    let result = repairer.join().expect("repairer").unwrap();
    holder.join().unwrap();
    assert!(result.counters.derived_state_published);
    assert!(result.counters.derived_repair);
}
