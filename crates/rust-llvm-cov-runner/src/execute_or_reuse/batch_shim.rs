use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::execute_or_reuse::batch_process_tree::{ProcessGroupIdentity, identity_still_valid};

#[path = "batch_shim_child.rs"]
mod batch_shim_child;
#[path = "batch_shim_list.rs"]
mod batch_shim_list;
#[path = "batch_shim_signal.rs"]
mod batch_shim_signal;
#[path = "batch_shim_write.rs"]
mod batch_shim_write;

pub(crate) use batch_shim_child::run_target_runner_shim_inner;
#[cfg(test)]
pub(crate) use batch_shim_signal::{
    ShimSignalForwarder, clear_shim_signal_forwarder, install_shim_signal_forwarder,
    trigger_shim_forward_signal_for_test,
};
#[cfg(test)]
pub(crate) use batch_shim_write::{write_shim_metadata, write_shim_start_metadata};

pub use crate::plan::batch_plan_shim_const::TARGET_RUNNER_SHIM_SUBCOMMAND;
pub(crate) const SHIM_START_SCHEMA: &str = "kiss-rust-llvm-cov-shim-start-v1";
pub(crate) const DELEGATED_START_SCHEMA: &str = "kiss-rust-llvm-cov-shim-delegated-start-v1";
pub(crate) const SHIM_LIST_SCHEMA: &str = "kiss-rust-llvm-cov-shim-list-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchShimStartMetadata {
    pub schema_version: String,
    pub id: String,
    pub shim_identity: ProcessGroupIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchShimDelegatedStartMetadata {
    pub schema_version: String,
    pub id: String,
    pub delegated_identity: ProcessGroupIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchShimMetadata {
    pub schema_version: String,
    pub id: String,
    pub full_name: String,
    pub profile_path: PathBuf,
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub spawn_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shim_identity: Option<ProcessGroupIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_identity: Option<ProcessGroupIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_frame_count: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchShimListMetadata {
    pub schema_version: String,
    pub id: String,
    pub binary_id: String,
    pub argv: Vec<String>,
    pub test_names: Vec<String>,
}

pub fn run_target_runner_shim(
    output_dir: &Path,
    runner_map: &Path,
    platform: &str,
    command: &[OsString],
) -> i32 {
    match run_target_runner_shim_inner(output_dir, runner_map, platform, command) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("kiss rust llvm-cov target runner: {err}");
            1
        }
    }
}

pub(crate) fn load_target_runner_shim_metadata(
    output_dir: &Path,
) -> io::Result<Vec<BatchShimMetadata>> {
    if !output_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut metadata = Vec::new();
    for entry in fs::read_dir(output_dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_completion_metadata_path(name) {
            continue;
        }
        let bytes = fs::read(path)?;
        metadata.push(serde_json::from_slice(&bytes).map_err(io::Error::other)?);
    }
    metadata.sort_by(|left: &BatchShimMetadata, right| left.full_name.cmp(&right.full_name));
    Ok(metadata)
}

pub(crate) fn load_target_runner_list_metadata(
    output_dir: &Path,
) -> io::Result<Vec<BatchShimListMetadata>> {
    if !output_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut metadata = Vec::new();
    for entry in fs::read_dir(output_dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.ends_with(".list.json") {
            continue;
        }
        let bytes = fs::read(path)?;
        metadata.push(serde_json::from_slice(&bytes).map_err(io::Error::other)?);
    }
    metadata.sort_by(|left: &BatchShimListMetadata, right| left.id.cmp(&right.id));
    Ok(metadata)
}

pub(crate) fn load_live_shim_process_identities(
    output_dir: &Path,
) -> io::Result<Vec<ProcessGroupIdentity>> {
    if !output_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut identities = Vec::new();
    for entry in fs::read_dir(output_dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let identity = if name.ends_with(".shim-start.json") {
            let bytes = fs::read(&path)?;
            let metadata: BatchShimStartMetadata =
                serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            metadata.shim_identity
        } else if name.ends_with(".delegated-start.json") {
            let bytes = fs::read(&path)?;
            let metadata: BatchShimDelegatedStartMetadata =
                serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            metadata.delegated_identity
        } else {
            continue;
        };
        if identity_still_valid(&identity) {
            identities.push(identity);
        }
    }
    Ok(identities)
}

fn is_completion_metadata_path(name: &str) -> bool {
    name.ends_with(".json")
        && !name.ends_with(".shim-start.json")
        && !name.ends_with(".delegated-start.json")
        && !name.ends_with(".list.json")
        && !name.ends_with(".json.tmp")
}

#[cfg(test)]
#[path = "batch_shim_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_shim_extra_test.rs"]
mod extra_tests;
