use super::*;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::test_support::witness_batch_tools;
use std::fs;

struct IdentityHarness {
    req: RustCoverageBatchRequest,
    plan: RustCoverageBatchPlan,
    tools: RustCoverageToolIdentity,
    _tmp: tempfile::TempDir,
}

fn identity_harness() -> IdentityHarness {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    req.generated_config = req
        .cache_root
        .join("runs")
        .join("run-a")
        .join("nextest.toml");
    let plan = crate::rust_llvm_cov_runner::build_rust_coverage_batch_plan(&req).unwrap();
    IdentityHarness {
        req,
        plan,
        tools: witness_batch_tools(),
        _tmp: tmp,
    }
}

fn seed_target(plan: &RustCoverageBatchPlan, nbytes: usize) {
    fs::create_dir_all(&plan.build_target).unwrap();
    fs::write(plan.build_target.join("artifact"), vec![0_u8; nbytes]).unwrap();
}

fn loaded_identity(cache_root: &std::path::Path) -> BuildIdentityFile {
    serde_json::from_slice(&fs::read(build_identity_path(cache_root)).unwrap()).unwrap()
}

#[test]
fn missing_marker_removes_cache_owned_target_and_writes_zero_baseline() {
    let h = identity_harness();
    seed_target(&h.plan, 8);
    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
    let marker = loaded_identity(&h.req.cache_root);
    assert_eq!(marker.input, build_identity_input(&h.req, &h.tools));
    assert_eq!(marker.build_target_baseline_bytes, 0);
}

#[test]
fn mismatched_marker_replaces_target_with_expected_zero_baseline() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    h.req.cargo_args.push("--features=old".to_string());
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req.cargo_args.clear();
    h.req.cargo_args.push("--features=new".to_string());
    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
    let marker = loaded_identity(&h.req.cache_root);
    assert_eq!(marker.input, build_identity_input(&h.req, &h.tools));
    assert_eq!(marker.build_target_baseline_bytes, 0);
}

#[test]
fn matching_zero_baseline_retains_partial_target() {
    let h = identity_harness();
    seed_target(&h.plan, 8);
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(h.plan.build_target.join("artifact").is_file());
}

#[test]
fn matching_marker_above_growth_limit_resets_zero_baseline() {
    let h = identity_harness();
    seed_target(&h.plan, 10);
    update_build_target_baseline(&h.req, &h.tools, &h.plan, 0).unwrap();
    fs::write(h.plan.build_target.join("artifact"), vec![0_u8; 20]).unwrap();
    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
    assert_eq!(
        loaded_identity(&h.req.cache_root).build_target_baseline_bytes,
        0
    );
}

#[test]
fn completion_update_records_target_size_without_changing_input() {
    let h = identity_harness();
    seed_target(&h.plan, 5);
    prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    seed_target(&h.plan, 5);
    let expected = build_identity_input(&h.req, &h.tools);
    let baseline = update_build_target_baseline(&h.req, &h.tools, &h.plan, 0).unwrap();
    let marker = loaded_identity(&h.req.cache_root);
    assert_eq!(baseline, 5);
    assert_eq!(marker.build_target_baseline_bytes, 5);
    assert_eq!(marker.input, expected);
}

#[test]
fn changed_context_after_interruption_replaces_target_and_marker() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req.cargo_args.push("--features=changed".to_string());
    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
    let marker = loaded_identity(&h.req.cache_root);
    assert_eq!(marker.input, build_identity_input(&h.req, &h.tools));
    assert_eq!(marker.build_target_baseline_bytes, 0);
}

#[test]
fn malformed_marker_fails_without_deleting_target_or_replacing() {
    let h = identity_harness();
    seed_target(&h.plan, 8);
    fs::create_dir_all(build_identity_path(&h.req.cache_root).parent().unwrap()).unwrap();
    fs::write(build_identity_path(&h.req.cache_root), b"{not-json").unwrap();
    let err = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    assert!(h.plan.build_target.join("artifact").is_file());
    assert_eq!(
        fs::read(build_identity_path(&h.req.cache_root)).unwrap(),
        b"{not-json"
    );
}
