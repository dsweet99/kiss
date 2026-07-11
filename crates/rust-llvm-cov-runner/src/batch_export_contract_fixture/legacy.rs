use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::batch_events::BatchCompilerArtifact;
use crate::batch_export::object_paths_for_executable;
use crate::batch_runner_resolve::resolve_batch_request_runners;
use crate::batch_shim::load_target_runner_shim_metadata;
use crate::{
    RustCoverageBatchRequest, RustCoverageToolIdentity, RustLlvmCov, RustLlvmCovOutcome,
    RustLlvmCovRequest, subprocess_cargo_llvm_cov_runner,
};

use super::oracle::{FIXTURE_ROOT, HELPER_BIN_ENV, fixture_cargo_args};

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
        // SAFETY: real-tool parity tests hold TARGET_RUNNER_ENV_LOCK while the
        // process-wide shim override is set, and restore the variable in Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: this restores the variable modified by EnvVarGuard::set.
        unsafe {
            if let Some(prior) = &self.prior {
                std::env::set_var(self.key, prior);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

pub(crate) fn legacy_request(
    cache_root: PathBuf,
    selector: &str,
    tools: &RustCoverageToolIdentity,
    helper_bin: &Path,
) -> RustLlvmCovRequest {
    RustLlvmCovRequest {
        selector: selector.to_string(),
        cwd: PathBuf::from(FIXTURE_ROOT),
        source_root: PathBuf::from(FIXTURE_ROOT),
        cargo: PathBuf::from("cargo"),
        llvm_cov_version: tools.llvm_cov_version.clone(),
        rustc_version: tools.rustc_version.clone(),
        cargo_args: fixture_cargo_args(),
        test_args: Vec::new(),
        env: BTreeMap::from([(
            HELPER_BIN_ENV.to_string(),
            helper_bin.to_string_lossy().to_string(),
        )]),
        cache_root,
        force_rerun: true,
        worker_slot: 0,
    }
}

pub(crate) fn run_legacy_selector(
    selector: &str,
    tools: &RustCoverageToolIdentity,
    helper_bin: &Path,
    cache_root: PathBuf,
) -> RustLlvmCovOutcome {
    let runner = RustLlvmCov::new(subprocess_cargo_llvm_cov_runner());
    runner
        .run_or_reuse(legacy_request(cache_root, selector, tools, helper_bin))
        .unwrap_or_else(|err| panic!("legacy llvm-cov selector `{selector}` failed: {err:?}"))
}

pub(crate) fn batch_request(
    tmp: &Path,
    selectors: &[String],
    helper_bin: &Path,
) -> RustCoverageBatchRequest {
    let mut req = RustCoverageBatchRequest {
        cwd: PathBuf::from(FIXTURE_ROOT),
        source_root: PathBuf::from(FIXTURE_ROOT),
        cargo: PathBuf::from("cargo"),
        cache_root: tmp.join("batch-cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: fixture_cargo_args(),
        test_args: Vec::new(),
        env: BTreeMap::from([(
            HELPER_BIN_ENV.to_string(),
            helper_bin.to_string_lossy().to_string(),
        )]),
        force_rerun: true,
        jobs: 2,
        generated_config: tmp.join("batch-cache/runs/parity/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners: BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
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
    assert_eq!(
        legacy.exit_code, batch.exit_code,
        "exit code differs for {selector}"
    );
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
    let plan_env = crate::batch_plan::build_rust_coverage_batch_plan(req)
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
mod legacy_coverage_tests {
    use super::*;
    use crate::RustCoverageToolIdentity;

    #[test]
    fn legacy_request_and_selector_helpers_are_constructible() {
        let tools = RustCoverageToolIdentity {
            cargo_version: "cargo".to_string(),
            llvm_cov_version: "llvm-cov".to_string(),
            rustc_version: "rustc".to_string(),
            cargo_nextest_version: "nextest".to_string(),
        };
        let req = legacy_request(
            PathBuf::from("/tmp/cache"),
            "alpha",
            &tools,
            Path::new("/tmp/helper"),
        );
        assert_eq!(req.selector, "alpha");
        let batch = batch_request(
            Path::new("/tmp"),
            &["alpha".to_string()],
            Path::new("/tmp/helper"),
        );
        assert_eq!(batch.logical_selectors, vec!["alpha".to_string()]);
    }
}
