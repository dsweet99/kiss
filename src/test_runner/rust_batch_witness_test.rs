use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_llvm_cov_runner::{
    AggregationCounters, BatchCompilerArtifact, ExportTools, InstanceResult,
    RustCoverageBatchRequest, RustCoverageToolIdentity, RustLineCoverage,
    aggregate_logical_selectors, aggregate_selectors_for_test, batch_identity, entry_fingerprint,
    is_cargo_config_input_path, object_paths_from_artifacts, parse_batch_event_stream,
    resolve_export_tools_from_env, resolve_export_tools_from_rustc, run_target_runner_shim,
    rust_cov_input_files, selector_matches_test, workspace_input_digest,
};

use super::rust_batch_witness_derived_test::witness_batch_derived;

#[test]
fn rust_batch_public_surface_is_exercised_from_kiss_tests() {
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_repo(tmp.path());
    let mut req = sample_batch_request(tmp.path());
    req.population_publication_selectors = Some(vec!["alpha".to_string()]);
    let tools = sample_tools();
    witness_batch_identity_and_input(tmp.path(), &req, &tools);
    witness_batch_events();
    witness_batch_export();
    witness_batch_derived(tmp.path(), &req, &tools);
    witness_tools_and_shim(tmp.path());
}

fn write_minimal_repo(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(root.join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
}

pub(super) fn sample_batch_request(root: &Path) -> RustCoverageBatchRequest {
    let (delegated_runners, runner_map_fingerprint, host_platform) =
        rust_llvm_cov_runner::placeholder_delegated_runner_fields();
    RustCoverageBatchRequest {
        cwd: root.to_path_buf(),
        source_root: root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: root.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: vec!["alpha".to_string()],
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
        force_rerun: false,
        jobs: 1,
        generated_config: root.join(".kiss/rust_llvm_cov_cache/runs/run-a/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners,
        runner_map_fingerprint,
        host_platform,
        coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        selector_timeout_millis: std::collections::BTreeMap::new(),
    }
}

pub(super) fn sample_tools() -> RustCoverageToolIdentity {
    RustCoverageToolIdentity {
        cargo_version: "cargo 1.88".to_string(),
        llvm_cov_version: "cargo-llvm-cov 0.8".to_string(),
        rustc_version: "rustc 1.88".to_string(),
        cargo_nextest_version: "cargo-nextest 0.9".to_string(),
    }
}

fn witness_batch_identity_and_input(
    root: &Path,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) {
    let identity = batch_identity(req, tools).unwrap();
    let _digest = workspace_input_digest(root).unwrap();
    let _files = rust_cov_input_files(root).unwrap();
    assert!(is_cargo_config_input_path(Path::new(".cargo/config.toml")));
    let _entry = entry_fingerprint(&identity.input_digest, req, tools, "alpha");
}

fn witness_batch_events() {
    let stdout = br#"{"reason":"compiler-artifact","executable":"/tmp/bin","filenames":["/tmp/a.o"]}
{"reason":"build-finished","success":true}
{"type":"test","event":"ok","name":"pkg::bin$alpha","exec_time":0.001}
{"type":"test","event":"failed","name":"pkg::bin$beta","exec_time":0.003,"stdout":"boom","reason":"assertion failed"}
"#;
    let parsed = parse_batch_event_stream(stdout).unwrap();
    assert_eq!(parsed.terminal_tests.len(), 2);
    assert_eq!(
        parsed.terminal_tests[1].reason.as_deref(),
        Some("assertion failed")
    );
    assert!(selector_matches_test("pkg::bin$alpha", "alpha", false));
    assert_eq!(
        aggregate_selectors_for_test("pkg::bin$alpha", &["alpha".to_string()], false),
        vec!["alpha".to_string()]
    );
}

fn witness_batch_export() {
    let objects = object_paths_from_artifacts(&[BatchCompilerArtifact {
        executable: Some("/tmp/bin".to_string()),
        filenames: vec!["/tmp/a.o".to_string()],
        nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
    }]);
    assert_eq!(objects, vec![PathBuf::from("/tmp/a.o")]);

    let instances = vec![InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        timed_out: false,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    }];
    let (outcomes, counters) =
        aggregate_logical_selectors(&["alpha".to_string()], false, &instances);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        counters,
        AggregationCounters {
            unmatched_selectors: 0,
            test_instances: 1,
        }
    );
}

fn witness_tools_and_shim(root: &Path) {
    let _env_tools = resolve_export_tools_from_env().unwrap();
    let _rustc_tools = resolve_export_tools_from_rustc(OsStr::new("rustc")).unwrap();
    let _ = ExportTools {
        llvm_cov: PathBuf::from("llvm-cov"),
        llvm_profdata: PathBuf::from("llvm-profdata"),
        llvm_readobj: PathBuf::from("llvm-readobj"),
    };

    let script = root.join("child.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 3\n").unwrap();
    make_executable(&script);
    let runner_map = root.join("runner-map.json");
    std::fs::write(&runner_map, b"{}").unwrap();
    let code = run_target_runner_shim(
        &root.join("instances"),
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.into_os_string()],
    );
    assert_eq!(code, 3);
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
