use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageToolIdentity;
use crate::rust_llvm_cov_runner::plan::batch_plan::{
    RustCoverageBatchPlan, RustCoverageBatchRequest,
};
use crate::rust_llvm_cov_runner::{BATCH_EXECUTION_POLICY_VERSION, CACHE_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuildIdentityPreparation {
    pub(crate) previous_baseline_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildIdentityFile {
    pub(crate) input: BuildIdentityInput,
    pub(crate) build_target_baseline_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildIdentityInput {
    pub(crate) cache_schema: String,
    pub(crate) execution_policy: String,
    pub(crate) tool_versions: [String; 4],
    pub(crate) source_root: String,
    pub(crate) cargo_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

const BUILD_TARGET_GROWTH_NUMERATOR: u64 = 3;
const BUILD_TARGET_GROWTH_DENOMINATOR: u64 = 2;

pub(crate) fn prepare_build_target_for_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
) -> io::Result<BuildIdentityPreparation> {
    let expected = build_identity_input(req, tools);
    let cache_owned = plan.build_target.starts_with(&req.cache_root);
    if let Some(previous) = load_build_identity(&req.cache_root)?
        && previous.input == expected
    {
        return retain_matching_or_reset_if_grown(req, tools, plan, previous, cache_owned);
    }
    reset_cache_owned_target_for_expected_context(req, tools, plan, cache_owned)
}

fn retain_matching_or_reset_if_grown(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
    previous: BuildIdentityFile,
    cache_owned: bool,
) -> io::Result<BuildIdentityPreparation> {
    let baseline = previous.build_target_baseline_bytes;
    if baseline > 0 && cache_owned {
        let current_bytes = path_size_bytes(&plan.build_target)?;
        let growth_limit = baseline.saturating_mul(BUILD_TARGET_GROWTH_NUMERATOR)
            / BUILD_TARGET_GROWTH_DENOMINATOR;
        if current_bytes > growth_limit {
            return reset_cache_owned_target_for_expected_context(req, tools, plan, cache_owned);
        }
    }
    Ok(BuildIdentityPreparation {
        previous_baseline_bytes: if cache_owned { baseline } else { 0 },
    })
}

fn reset_cache_owned_target_for_expected_context(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
    cache_owned: bool,
) -> io::Result<BuildIdentityPreparation> {
    if cache_owned {
        remove_build_target(&plan.build_target)?;
        write_expected_zero_baseline_marker(req, tools)?;
    }
    Ok(BuildIdentityPreparation {
        previous_baseline_bytes: 0,
    })
}

fn write_expected_zero_baseline_marker(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> io::Result<()> {
    write_build_identity_atomic(
        &req.cache_root,
        &BuildIdentityFile {
            input: build_identity_input(req, tools),
            build_target_baseline_bytes: 0,
        },
    )
}

pub(crate) fn update_build_target_baseline(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
    previous_baseline_bytes: u64,
) -> io::Result<u64> {
    if previous_baseline_bytes != 0 {
        return Ok(previous_baseline_bytes);
    }
    let cache_owned = plan.build_target.starts_with(&req.cache_root);
    let current_target_bytes = if cache_owned {
        path_size_bytes(&plan.build_target)?
    } else {
        0
    };
    let input = match load_build_identity(&req.cache_root)? {
        Some(previous) => previous.input,
        None => build_identity_input(req, tools),
    };
    write_build_identity_atomic(
        &req.cache_root,
        &BuildIdentityFile {
            input,
            build_target_baseline_bytes: current_target_bytes,
        },
    )?;
    Ok(current_target_bytes)
}

pub(crate) fn build_identity_input(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> BuildIdentityInput {
    let env =
        crate::rust_llvm_cov_runner::plan::batch_plan::effective_coverage_identity_environment(req);
    BuildIdentityInput {
        cache_schema: CACHE_SCHEMA_VERSION.to_string(),
        execution_policy: BATCH_EXECUTION_POLICY_VERSION.to_string(),
        tool_versions: [
            tools.cargo_version.clone(),
            tools.llvm_cov_version.clone(),
            tools.rustc_version.clone(),
            tools.cargo_nextest_version.clone(),
        ],
        source_root: req.source_root.to_string_lossy().to_string(),
        cargo_args: req.cargo_args.clone(),
        env,
    }
}

fn load_build_identity(cache_root: &Path) -> io::Result<Option<BuildIdentityFile>> {
    let path = build_identity_path(cache_root);
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(io::Error::other),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_build_identity_atomic(cache_root: &Path, marker: &BuildIdentityFile) -> io::Result<()> {
    let path = build_identity_path(cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("build identity path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(marker).map_err(io::Error::other)?,
    )?;
    fs::rename(tmp, path)
}

pub(crate) fn build_identity_path(cache_root: &Path) -> PathBuf {
    cache_root.join("build").join("identity.json")
}

fn remove_build_target(build_target: &Path) -> io::Result<()> {
    match fs::remove_dir_all(build_target) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn path_size_bytes(path: &Path) -> io::Result<u64> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err),
    };
    if meta.is_file() {
        return Ok(meta.len());
    }
    if meta.is_dir() {
        return fs::read_dir(path)?.try_fold(0, |total, entry| {
            Ok(total + path_size_bytes(&entry?.path())?)
        });
    }
    Ok(0)
}

#[cfg(test)]
#[path = "batch_run_identity_test.rs"]
mod identity_tests;
