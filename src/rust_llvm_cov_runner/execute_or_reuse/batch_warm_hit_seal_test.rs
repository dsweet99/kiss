use super::*;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::publish_next_entry_state;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn identity(generation: &str) -> RustCoverageBatchIdentity {
    RustCoverageBatchIdentity {
        input_digest: "input".into(),
        generation_fingerprint: generation.into(),
        selection_context_fingerprint: "selctx".into(),
        ordinary_source_digests: BTreeMap::new(),
    }
}

fn request(cache_root: PathBuf, selectors: &[&str]) -> RustCoverageBatchRequest {
    RustCoverageBatchRequest {
        cwd: PathBuf::from("."),
        source_root: PathBuf::from("."),
        cargo: PathBuf::from("cargo"),
        cache_root,
        logical_selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
        force_rerun: false,
        jobs: 1,
        generated_config: PathBuf::from("cfg"),
        population_publication_selectors: None,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
        coverage_output_mode: crate::rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        selector_timeout_millis: BTreeMap::new(),
    }
}

#[test]
fn try_warm_all_hit_seal_rejects_all_passed_false() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().to_path_buf();
    let id = identity("gen1");
    publish_next_entry_state(&cache_root, "gen1", "entries1").unwrap();
    let req = request(cache_root.clone(), &["a::t"]);
    write_warm_all_hit_seal(&req, &id).unwrap();
    let path = seal_path(&cache_root);
    let mut seal: WarmAllHitSeal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    seal.all_passed = false;
    fs::write(&path, serde_json::to_vec(&seal).unwrap()).unwrap();
    assert_eq!(try_warm_all_hit_seal(&req, &id), None);
}

#[test]
fn try_warm_all_hit_seal_rejects_mismatched_fingerprints() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().to_path_buf();
    let id = identity("gen1");
    publish_next_entry_state(&cache_root, "gen1", "entries1").unwrap();
    let req = request(cache_root, &["a::t"]);
    write_warm_all_hit_seal(&req, &id).unwrap();
    let other = identity("gen-other");
    assert_eq!(try_warm_all_hit_seal(&req, &other), None);
}

#[test]
fn write_and_try_warm_all_hit_seal_positive_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().to_path_buf();
    let id = identity("gen1");
    publish_next_entry_state(&cache_root, "gen1", "entries1").unwrap();
    let req = request(cache_root, &["a::t", "b::t"]);
    write_warm_all_hit_seal(&req, &id).unwrap();
    assert_eq!(try_warm_all_hit_seal(&req, &id), Some(()));
}

#[test]
fn write_warm_all_hit_seal_errors_without_entry_state() {
    let tmp = tempfile::tempdir().unwrap();
    let req = request(tmp.path().to_path_buf(), &["a::t"]);
    let err = write_warm_all_hit_seal(&req, &identity("gen1")).unwrap_err();
    assert!(err.to_string().contains("entry_state"));
}
