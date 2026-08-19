use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::execute_or_reuse::batch_events::BatchCompilerArtifact;
use crate::execute_or_reuse::batch_export::object_paths_for_executable;
use crate::plan::batch_runner_resolve::resolve_batch_request_runners;
use crate::execute_or_reuse::batch_shim::load_target_runner_shim_metadata;
use crate::execute_or_reuse::llvm_cov_json::parse_llvm_cov_json_file;
use crate::{
    RustCovCacheStatus, RustCoverageBatchRequest, RustCoverageToolIdentity, RustLineCoverage,
    RustLlvmCovOutcome,
};

use super::oracle::{FIXTURE_ROOT, HELPER_BIN_ENV, extract_json_payload, fixture_cargo_args};

pub(crate) fn discover_compiler_artifacts(
    executable: &Path,
    seeds: &[PathBuf],
) -> Vec<BatchCompilerArtifact> {
    vec![BatchCompilerArtifact {
        executable: Some(executable.to_string_lossy().into_owned()),
        filenames: seeds
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        nextest_binary_id: None,
    libtest_binary_prefix: None,
    src_path: None,
    is_test_harness: false,
    }]
}

pub(crate) fn collect_object_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_object_files(&path, out);
            continue;
        }
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if matches!(ext, "o" | "rlib" | "rmeta") {
            out.push(path);
        }
    }
}

pub(crate) fn fixture_relative_coverage(
    coverage: &crate::RustLineCoverage,
    fixture_root: &Path,
) -> BTreeMap<String, BTreeSet<u32>> {
    let canonical_root = fixture_root
        .canonicalize()
        .unwrap_or_else(|_| fixture_root.to_path_buf());
    let mut filtered = BTreeMap::new();
    for (file, lines) in &coverage.files {
        let path = PathBuf::from(file);
        let canonical = path.canonicalize().unwrap_or(path);
        if !canonical.starts_with(&canonical_root) {
            continue;
        }
        let rel = canonical
            .strip_prefix(&canonical_root)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .replace('\\', "/");
        if !lines.is_empty() {
            filtered.insert(rel, lines.clone());
        }
    }
    filtered
}

pub(crate) struct EnvVarGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &Path) -> Self {
        let prior = std::env::var_os(key);


        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {

        unsafe {
            if let Some(prior) = &self.prior {
                std::env::set_var(self.key, prior);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

pub(crate) fn run_legacy_selector(
    selector: &str,
    _tools: &RustCoverageToolIdentity,
    helper_bin: &Path,
    cache_root: PathBuf,
) -> RustLlvmCovOutcome {
    run_legacy_selector_with_args(selector, _tools, helper_bin, cache_root, &[])
}

pub(crate) fn run_legacy_selector_with_args(
    selector: &str,
    _tools: &RustCoverageToolIdentity,
    helper_bin: &Path,
    cache_root: PathBuf,
    test_args: &[String],
) -> RustLlvmCovOutcome {
    run_per_selector_cargo_llvm_cov_oracle(selector, helper_bin, &cache_root, test_args)
}

fn run_per_selector_cargo_llvm_cov_oracle(
    selector: &str,
    helper_bin: &Path,
    cache_root: &Path,
    test_args: &[String],
) -> RustLlvmCovOutcome {
    let started = Instant::now();
    let oracle_root = cache_root.join("oracle");
    let target_dir = oracle_root.join("target");
    let artifact_path = oracle_root.join("coverage.json");
    fs::create_dir_all(&target_dir).expect("create oracle target dir");
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).expect("create oracle artifact parent");
    }

    let cargo = oracle_cargo_program();
    let mut command = Command::new(&cargo);
    command
        .arg("llvm-cov")
        .arg("test")
        .arg("--json")
        .arg("--output-path")
        .arg(&artifact_path)
        .arg("--no-clean");
    for arg in fixture_cargo_args() {
        command.arg(arg);
    }
    command.arg(selector).arg("--");
    for arg in test_args {
        command.arg(arg);
    }
    command
        .current_dir(FIXTURE_ROOT)
        .env(HELPER_BIN_ENV, helper_bin)
        .env("CARGO_TARGET_DIR", &target_dir);

    let output = command
        .output()
        .unwrap_or_else(|err| panic!("cargo llvm-cov oracle selector `{selector}` failed: {err}"));
    let status = rpytest_runner::TestStatus::from_exit_status(output.status);
    let exit_code = output.status.code();
    let source_root = PathBuf::from(FIXTURE_ROOT).join("runner");
    let coverage = if status == rpytest_runner::TestStatus::Passed {
        if artifact_path.is_file() {
            parse_llvm_cov_json_file(&artifact_path, &source_root)
                .unwrap_or_else(|_| parse_oracle_stdout_coverage(&output.stdout, &source_root))
        } else {
            parse_oracle_stdout_coverage(&output.stdout, &source_root)
        }
    } else {
        RustLineCoverage {
            files: BTreeMap::new(),
        }
    };

    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status,
        exit_code,
        duration: started.elapsed(),
        coverage,
        test_binary_ids: vec!["test-bin".to_string()],
        cache_status: RustCovCacheStatus::FreshUnstored,
        stdout: Some(output.stdout),
        stderr: Some(output.stderr),
    }
}

fn oracle_cargo_program() -> PathBuf {
    std::env::var_os("KISS_ORACLE_CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

fn parse_oracle_stdout_coverage(stdout: &[u8], source_root: &Path) -> RustLineCoverage {
    if stdout.is_empty() {
        return RustLineCoverage {
            files: BTreeMap::new(),
        };
    }
    let payload = extract_json_payload(stdout);
    crate::execute_or_reuse::llvm_cov_json::parse_llvm_cov_json(&payload, source_root).unwrap_or(RustLineCoverage {
        files: BTreeMap::new(),
    })
}

#[cfg(test)]
pub(crate) fn parse_oracle_stdout_coverage_for_test(
    stdout: &[u8],
    source_root: &Path,
) -> RustLineCoverage {
    parse_oracle_stdout_coverage(stdout, source_root)
}

pub(crate) fn batch_request(
    tmp: &Path,
    selectors: &[String],
    helper_bin: &Path,
) -> RustCoverageBatchRequest {
    batch_request_with_args(tmp, selectors, helper_bin, &[], 2)
}

pub(crate) fn batch_request_with_args(
    tmp: &Path,
    selectors: &[String],
    helper_bin: &Path,
    test_args: &[String],
    jobs: usize,
) -> RustCoverageBatchRequest {
    let mut req = RustCoverageBatchRequest {
        cwd: PathBuf::from(FIXTURE_ROOT),
        source_root: PathBuf::from(FIXTURE_ROOT),
        cargo: PathBuf::from("cargo"),
        cache_root: tmp.join("batch-cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: fixture_cargo_args(),
        test_args: test_args.to_vec(),
        env: BTreeMap::from([(
            HELPER_BIN_ENV.to_string(),
            helper_bin.to_string_lossy().to_string(),
        )]),
        force_rerun: true,
jobs,
        generated_config: tmp.join("batch-cache/runs/parity/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
        coverage_output_mode: crate::CoverageOutputMode::SelectorEntries,
        selector_timeout_millis: std::collections::BTreeMap::new(),
    };
    resolve_batch_request_runners(&mut req).expect("resolve delegated runners");
    req
}

pub(crate) fn assert_outcomes_match(
    selector: &str,
    legacy: &RustLlvmCovOutcome,
    batch: &RustLlvmCovOutcome,
    fixture_root: &Path,
    debug: &str,
) {
    assert_eq!(legacy.status, batch.status, "status differs for {selector}");
    if legacy.status == rpytest_runner::TestStatus::Failed {
        if legacy.exit_code == Some(37) {
            assert_eq!(
                batch.exit_code,
                Some(1),
                "batch public failure exit must normalize to 1 for diagnostic child exit 37 on {selector}"
            );
        } else {
            assert_eq!(
                batch.exit_code,
                Some(1),
                "batch failure exit code differs for {selector}"
            );
        }
    } else {
        assert_eq!(
            legacy.exit_code, batch.exit_code,
            "exit code differs for {selector}"
        );
    }
    assert_eq!(
        fixture_relative_coverage(&legacy.coverage, fixture_root),
        fixture_relative_coverage(&batch.coverage, fixture_root),
        "fixture-relative covered lines differ for {selector}\n{debug}"
    );
}

pub(crate) fn batch_profile_debug(req: &RustCoverageBatchRequest) -> String {
    let Some(run_root) = req.generated_config.parent() else {
        return "batch debug: generated config has no parent".to_string();
    };
    let plan_env = crate::plan::batch_plan::build_rust_coverage_batch_plan(req)
        .ok()
        .map(|plan| {
            plan.argv
                .windows(2)
                .filter(|args| args[0] == "--config")
                .map(|args| format!("--config {}", args[1]))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let output_dir = run_root.join("instances");
    let Ok(metadata) = load_target_runner_shim_metadata(&output_dir) else {
        return format!(
            "batch debug: failed to read metadata from {}",
            output_dir.display()
        );
    };
    let mut lines = vec![format!(
        "batch debug: {} metadata records in {}",
        metadata.len(),
        output_dir.display()
    )];
    lines.extend(plan_env);
    for item in metadata {
        let profile_len = fs::metadata(&item.profile_path)
            .ok()
            .map(|meta| meta.len())
            .unwrap_or(0);
        lines.push(format!(
            "  {} profile={} bytes={} exit={:?}",
            item.full_name,
            item.profile_path.display(),
            profile_len,
            item.exit_code
        ));
    }
    lines.join("\n")
}

pub(crate) fn assert_export_uses_seed_objects_only(
    artifacts: &[BatchCompilerArtifact],
    executable: &Path,
    catalog_len: usize,
) {
    let seeds = object_paths_for_executable(artifacts, executable);
    assert!(!seeds.is_empty(), "expected non-empty seed objects");
    assert!(
        seeds.len() < catalog_len,
        "seed set ({}) must be smaller than full catalog ({catalog_len})",
        seeds.len()
    );
}

pub(crate) fn assert_direct_export_matches_oracle(
    oracle_lines: &BTreeMap<String, BTreeSet<u32>>,
    direct_lines: &BTreeMap<String, BTreeSet<u32>>,
    seeds: &[PathBuf],
    catalog_len: usize,
) {
    assert!(
        direct_lines.contains_key("runner/src/lib.rs"),
        "subprocess test should cover runner/src/lib.rs; got keys {:?}",
        direct_lines.keys().collect::<Vec<_>>()
    );
    assert!(
        seeds
            .iter()
            .any(|path| { path.to_string_lossy().contains("export_contract_helper") }),
        "seed objects must include cross-package helper artifact: {seeds:?}"
    );
    for (file, lines) in oracle_lines {
        assert_eq!(
            direct_lines.get(file),
            Some(lines),
            "oracle file `{file}` must match direct export lines"
        );
    }
    assert!(
        seeds.len() < catalog_len,
        "seed objects ({}) must be smaller than full catalog ({catalog_len})",
        seeds.len()
    );
}

#[cfg(test)]
#[path = "legacy_fixture_test.rs"]
mod legacy_coverage_tests;
