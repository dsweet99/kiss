use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::{
    DerivedPublishCounters, RustCovCacheEntry, RustCovCacheStatus, RustCoverageBatchRequest,
    RustCoverageToolIdentity, RustLineCoverage, RustLlvmCovOutcome, RustPopulationState,
    batch_identity, entry_fingerprint, generation_entries_fingerprint,
    load_current_generation_line_index, load_current_population_state,
    population_derived_state_stale, publish_derived_state, repo_relative_coverage_file,
    repo_relative_path, store_rust_cov_cache_entry,
};

pub(super) fn witness_batch_derived(
    root: &Path,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) {
    let identity = batch_identity(req, tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, "alpha");
    let source = root.join("src").join("lib.rs");
    let mut coverage = BTreeMap::new();
    coverage.insert(source.to_string_lossy().to_string(), BTreeSet::from([1]));
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage { files: coverage },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
    std::fs::write(req.cache_root.join("entries/ignore.txt"), b"x").unwrap();
    std::fs::write(req.cache_root.join("entries/bad.json"), b"{").unwrap();
    assert_eq!(
        repo_relative_path(root, &source).as_deref(),
        Some("src/lib.rs")
    );
    assert_eq!(
        repo_relative_coverage_file(root, &source.to_string_lossy()).as_deref(),
        Some("src/lib.rs")
    );
    assert!(population_derived_state_stale(req, tools, &identity).unwrap());
    let counters: DerivedPublishCounters =
        publish_derived_state(req, tools, &identity, &["alpha".to_string()], true).unwrap();
    assert!(counters.derived_repair);
    assert_eq!(
        generation_entries_fingerprint(&req.cache_root, &identity.generation_fingerprint).unwrap(),
        std::fs::read_to_string(req.cache_root.join("index.json"))
            .unwrap()
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("\"entries_fingerprint\": \"")
                    .and_then(|rest| rest.strip_suffix("\","))
            })
            .expect("entries fingerprint in index")
    );
    assert!(!population_derived_state_stale(req, tools, &identity).unwrap());
    witness_population_state_loaders(root, req, &identity);
    std::fs::write(req.cache_root.join("population.json"), b"not-json").unwrap();
    assert!(population_derived_state_stale(req, tools, &identity).unwrap());
    std::fs::write(
        req.cache_root.join("population.json"),
        r#"{"generation_fingerprint":"stale","selectors":["alpha"]}"#,
    )
    .unwrap();
    assert!(population_derived_state_stale(req, tools, &identity).unwrap());
}

fn witness_population_state_loaders(
    root: &Path,
    req: &RustCoverageBatchRequest,
    identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
) {
    let state: RustPopulationState = load_current_population_state(
        &req.cache_root,
        root,
        identity,
        Some(&["alpha".to_string()]),
    )
    .expect("population state after publish");
    assert_eq!(
        state.generation_fingerprint,
        identity.generation_fingerprint
    );
    assert!(!state.entries_fingerprint.is_empty());
    assert_eq!(state.selectors, vec!["alpha".to_string()]);
    assert!(state.line_index.contains_key("src/lib.rs"));
    let cloned = state.clone();
    assert_eq!(format!("{cloned:?}"), format!("{state:?}"));

    let index = load_current_generation_line_index(&req.cache_root, root)
        .expect("current generation line index");
    assert!(index.contains_key("src/lib.rs"));

    assert!(
        load_current_population_state(
            &req.cache_root,
            root,
            identity,
            Some(&["missing".to_string()]),
        )
        .is_none()
    );
    let other = tempfile::tempdir().unwrap();
    assert!(
        load_current_population_state(
            &req.cache_root,
            other.path(),
            identity,
            Some(&["alpha".to_string()]),
        )
        .is_none()
    );

    let literal = RustPopulationState {
        input_fingerprint: state.input_fingerprint.clone(),
        generation_fingerprint: state.generation_fingerprint.clone(),
        selection_context_fingerprint: state.selection_context_fingerprint.clone(),
        entries_fingerprint: state.entries_fingerprint.clone(),
        selectors: state.selectors.clone(),
        line_index: state.line_index.clone(),
    };
    assert_eq!(state, literal);
}

#[test]
fn rust_batch_derived_public_surface_is_exercised_from_kiss_tests() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let mut req = super::rust_batch_witness_test::sample_batch_request(tmp.path());
    req.population_publication_selectors = Some(vec!["alpha".to_string()]);
    let tools = super::rust_batch_witness_test::sample_tools();
    witness_batch_derived(tmp.path(), &req, &tools);
}
