use super::*;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::test_support::witness_batch_tools;
use std::fs;
use std::process::Command;

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

fn write_cargo_fixture(root: &std::path::Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='restart_reuse'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn answer() -> u32 { 42 }\n#[cfg(test)] mod tests { #[test] fn answers() { assert_eq!(super::answer(), 42); } }\n",
    )
    .unwrap();
}

fn run_llvm_cov_and_read_fresh(root: &std::path::Path, plan: &RustCoverageBatchPlan) -> Vec<bool> {
    let output = Command::new("cargo")
        .args([
            "llvm-cov",
            "nextest",
            "--no-report",
            "--cargo-message-format",
            "json",
            "--test-threads",
            "1",
        ])
        .current_dir(root)
        .envs(&plan.env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo llvm-cov failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["reason"] == "compiler-artifact")
        .filter_map(|value| value["fresh"].as_bool())
        .collect()
}

fn append_duplicate_path_entry(path: &str) -> String {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let first = path.split(separator).next().unwrap();
    format!("{path}{separator}{first}")
}

fn write_executable_stub(path: &std::path::Path, contents: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
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
fn duplicate_path_entry_retains_cargo_artifacts() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    let separator = if cfg!(windows) { ';' } else { ':' };
    let normalized = ["/one", "/two"].join(&separator.to_string());
    h.req.env.insert("PATH".into(), normalized.clone());
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req
        .env
        .insert("PATH".into(), append_duplicate_path_entry(&normalized));

    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(h.plan.build_target.join("artifact").is_file());
    let marker = loaded_identity(&h.req.cache_root);
    assert!(!marker.input.env.contains_key("PATH"));
    assert_eq!(marker.build_target_baseline_bytes, 0);
}

#[test]
fn path_resolution_change_replaces_cargo_artifacts() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    let old_bin = h.req.source_root.join("old-bin");
    let new_bin = h.req.source_root.join("new-bin");
    write_executable_stub(&old_bin.join("cmake"), b"old\n");
    write_executable_stub(&new_bin.join("cmake"), b"new\n");
    h.req
        .env
        .insert("PATH".into(), old_bin.to_string_lossy().into_owned());
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req
        .env
        .insert("PATH".into(), new_bin.to_string_lossy().into_owned());

    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
    let marker = loaded_identity(&h.req.cache_root);
    assert_eq!(
        marker.input.resolved_tools.get("cmake").map(String::as_str),
        Some(new_bin.join("cmake").to_str().unwrap())
    );
}

#[test]
fn unused_path_prefix_retains_cargo_artifacts() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    h.req.env.insert(
        "PATH".into(),
        "/home/dsweet/micromamba/envs/sameq/bin:/usr/bin".into(),
    );
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req.env.insert(
        "PATH".into(),
        "/home/dsweet/bin:/home/dsweet/.local/opt/node/bin:/home/dsweet/micromamba/envs/sameq/bin:/usr/bin".into(),
    );

    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(h.plan.build_target.join("artifact").is_file());
    assert!(
        !loaded_identity(&h.req.cache_root)
            .input
            .env
            .contains_key("PATH")
    );
}

#[test]
fn legacy_marker_with_changed_tool_path_replaces_cargo_artifacts() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    let old_bin = h.req.source_root.join("old-bin");
    let new_bin = h.req.source_root.join("new-bin");
    write_executable_stub(&old_bin.join("cmake"), b"old\n");
    write_executable_stub(&new_bin.join("cmake"), b"new\n");
    h.req
        .env
        .insert("PATH".into(), old_bin.to_string_lossy().into_owned());
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    let mut legacy = loaded_identity(&h.req.cache_root);
    legacy.input.resolved_tools.clear();
    write_build_identity_atomic(&h.req.cache_root, &legacy).unwrap();
    h.req
        .env
        .insert("PATH".into(), new_bin.to_string_lossy().into_owned());

    prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();

    assert!(!h.plan.build_target.exists());
    assert_eq!(
        loaded_identity(&h.req.cache_root)
            .input
            .resolved_tools
            .get("cmake")
            .map(String::as_str),
        Some(new_bin.join("cmake").to_str().unwrap())
    );
}

#[test]
fn inherited_profile_path_does_not_replace_cargo_artifacts() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    h.req
        .env
        .insert("LLVM_PROFILE_FILE".into(), "/outer/old.profraw".into());
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req
        .env
        .insert("LLVM_PROFILE_FILE".into(), "/outer/new.profraw".into());

    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(h.plan.build_target.join("artifact").is_file());
    assert_eq!(
        loaded_identity(&h.req.cache_root).input.env["LLVM_PROFILE_FILE"],
        h.req
            .source_root
            .join(".kiss/profraw/default_%m_%p.profraw")
            .to_string_lossy()
    );
}

#[test]
fn non_path_environment_mismatch_replaces_cargo_artifacts() {
    let mut h = identity_harness();
    seed_target(&h.plan, 8);
    h.req.env.insert("RUSTFLAGS".into(), "-Copt-level=1".into());
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    h.req.env.insert("RUSTFLAGS".into(), "-Copt-level=2".into());

    let prep = prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
}

#[test]
fn tool_identity_mismatch_replaces_cargo_artifacts() {
    let h = identity_harness();
    seed_target(&h.plan, 8);
    write_expected_zero_baseline_marker(&h.req, &h.tools).unwrap();
    let mut changed_tools = h.tools.clone();
    changed_tools.rustc_version.push_str(" changed");

    let prep = prepare_build_target_for_identity(&h.req, &changed_tools, &h.plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!h.plan.build_target.exists());
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
fn duplicate_path_launch_reuses_real_llvm_cov_cargo_artifacts() {
    let mut h = identity_harness();
    write_cargo_fixture(&h.req.source_root);
    let path = std::env::var("PATH").unwrap();
    h.req.env.insert("PATH".into(), path.clone());
    h.plan = crate::rust_llvm_cov_runner::build_rust_coverage_batch_plan(&h.req).unwrap();
    prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    let first = run_llvm_cov_and_read_fresh(&h.req.source_root, &h.plan);
    assert!(first.iter().any(|fresh| !fresh));
    assert_eq!(
        loaded_identity(&h.req.cache_root).build_target_baseline_bytes,
        0
    );

    h.req
        .env
        .insert("PATH".into(), append_duplicate_path_entry(&path));
    let second_plan = crate::rust_llvm_cov_runner::build_rust_coverage_batch_plan(&h.req).unwrap();
    assert_eq!(h.plan.env["PATH"], second_plan.env["PATH"]);
    prepare_build_target_for_identity(&h.req, &h.tools, &second_plan).unwrap();
    let second = run_llvm_cov_and_read_fresh(&h.req.source_root, &second_plan);

    assert!(!second.is_empty());
    assert!(second.iter().all(|fresh| *fresh));
}

#[test]
fn unused_path_prefix_reuses_real_llvm_cov_cargo_artifacts() {
    let mut h = identity_harness();
    write_cargo_fixture(&h.req.source_root);
    let path = std::env::var("PATH").unwrap();
    h.req.env.insert("PATH".into(), path.clone());
    h.plan = crate::rust_llvm_cov_runner::build_rust_coverage_batch_plan(&h.req).unwrap();
    prepare_build_target_for_identity(&h.req, &h.tools, &h.plan).unwrap();
    let first = run_llvm_cov_and_read_fresh(&h.req.source_root, &h.plan);
    assert!(first.iter().any(|fresh| !fresh));

    let unused = h.req.source_root.join("unused-bin");
    fs::create_dir_all(&unused).unwrap();
    let separator = if cfg!(windows) { ';' } else { ':' };
    h.req.env.insert(
        "PATH".into(),
        format!("{}{separator}{path}", unused.display()),
    );
    let second_plan = crate::rust_llvm_cov_runner::build_rust_coverage_batch_plan(&h.req).unwrap();
    prepare_build_target_for_identity(&h.req, &h.tools, &second_plan).unwrap();
    assert!(second_plan.build_target.join("debug").exists() || second_plan.build_target.exists());
    let second = run_llvm_cov_and_read_fresh(&h.req.source_root, &second_plan);

    assert!(!second.is_empty());
    assert!(second.iter().all(|fresh| *fresh));
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
