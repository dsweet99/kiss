use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::plan::batch_fingerprint::RustCoverageToolIdentity;
use crate::plan::batch_plan::{RustCoverageBatchPlan, RustCoverageBatchRequest};
use crate::{BATCH_EXECUTION_POLICY_VERSION, CACHE_SCHEMA_VERSION};
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
    let build_target_is_cache_owned = plan.build_target.starts_with(&req.cache_root);
    if let Some(previous) = load_build_identity(&req.cache_root)?
        && previous.input == expected
    {
        let baseline = previous.build_target_baseline_bytes;
        if baseline > 0 && build_target_is_cache_owned {
            let current_bytes = path_size_bytes(&plan.build_target)?;
            let growth_limit = baseline.saturating_mul(BUILD_TARGET_GROWTH_NUMERATOR)
                / BUILD_TARGET_GROWTH_DENOMINATOR;
            if current_bytes > growth_limit {
                remove_build_target(&plan.build_target)?;
                return Ok(BuildIdentityPreparation {
                    previous_baseline_bytes: 0,
                });
            }
        }
        return Ok(BuildIdentityPreparation {
            previous_baseline_bytes: if build_target_is_cache_owned {
                baseline
            } else {
                0
            },
        });
    }
    if build_target_is_cache_owned {
        remove_build_target(&plan.build_target)?;
    }
    Ok(BuildIdentityPreparation {
        previous_baseline_bytes: 0,
    })
}

pub(crate) fn publish_successful_build_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
    previous_baseline_bytes: u64,
) -> io::Result<u64> {
    let build_target_is_cache_owned = plan.build_target.starts_with(&req.cache_root);
    // Repo `target/` is retained across samples and can be tens of GB of
    // incremental objects. The growth cap only deletes cache-owned trees, so
    // do not recursively size the external target on the cold coverage path.
    let current_target_bytes = if build_target_is_cache_owned {
        path_size_bytes(&plan.build_target)?
    } else {
        0
    };
    let baseline_bytes = if previous_baseline_bytes == 0 {
        current_target_bytes
    } else {
        previous_baseline_bytes
    };
    let marker = BuildIdentityFile {
        input: build_identity_input(req, tools),
        build_target_baseline_bytes: baseline_bytes,
    };
    write_build_identity_atomic(&req.cache_root, &marker)?;
    Ok(baseline_bytes)
}

pub(crate) fn build_identity_input(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> BuildIdentityInput {
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
        env: req.env.clone(),
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
