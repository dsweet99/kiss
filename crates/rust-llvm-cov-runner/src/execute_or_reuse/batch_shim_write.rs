use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::execute_or_reuse::batch_process_tree::ProcessGroupIdentity;

use super::{
    BatchShimDelegatedStartMetadata, BatchShimListMetadata, BatchShimMetadata,
    BatchShimStartMetadata, DELEGATED_START_SCHEMA, SHIM_START_SCHEMA,
};

pub(crate) fn write_shim_metadata(
    output_dir: &Path,
    id: &str,
    metadata: &BatchShimMetadata,
) -> io::Result<()> {
    write_metadata_atomically(output_dir, &format!("{id}.json"), metadata)
}

pub(crate) fn write_shim_start_metadata(
    output_dir: &Path,
    id: &str,
    shim_identity: &ProcessGroupIdentity,
) -> io::Result<()> {
    let metadata = BatchShimStartMetadata {
        schema_version: SHIM_START_SCHEMA.to_string(),
        id: id.to_string(),
        shim_identity: shim_identity.clone(),
    };
    write_metadata_atomically(output_dir, &format!("{id}.shim-start.json"), &metadata)
}

pub(crate) fn write_delegated_start_metadata(
    output_dir: &Path,
    id: &str,
    delegated_identity: &ProcessGroupIdentity,
) -> io::Result<()> {
    let metadata = BatchShimDelegatedStartMetadata {
        schema_version: DELEGATED_START_SCHEMA.to_string(),
        id: id.to_string(),
        delegated_identity: delegated_identity.clone(),
    };
    write_metadata_atomically(output_dir, &format!("{id}.delegated-start.json"), &metadata)
}

pub(crate) fn write_shim_list_metadata(
    output_dir: &Path,
    id: &str,
    metadata: &BatchShimListMetadata,
) -> io::Result<()> {
    write_metadata_atomically(output_dir, &format!("{id}.list.json"), metadata)
}

fn write_metadata_atomically<T: Serialize>(
    output_dir: &Path,
    file_name: &str,
    metadata: &T,
) -> io::Result<()> {
    let metadata_path = output_dir.join(file_name);
    let tmp_path = output_dir.join(format!(".{file_name}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    serde_json::to_writer(&mut file, metadata).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, metadata_path)
}

pub(crate) fn instance_full_name(command: &[std::ffi::OsString]) -> String {
    if should_use_nextest_env_for_instance(command)
        && let (Some(binary_id), Some(test_name)) = (
            std::env::var("NEXTEST_BINARY_ID").ok(),
            std::env::var("NEXTEST_TEST_NAME").ok(),
        )
    {
        return format!("{binary_id}${test_name}");
    }
    if let Some((binary, test_name)) = exact_test_from_command(command) {
        let binary_id = binary
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| binary.to_string_lossy().to_string());
        return format!("{binary_id}${test_name}");
    }
    unique_shim_id()
}

pub(crate) fn list_binary_id(command: &[std::ffi::OsString]) -> String {
    if should_use_nextest_env_for_list(command)
        && let Some(binary_id) = std::env::var("NEXTEST_BINARY_ID").ok()
    {
        return binary_id;
    }
    command
        .first()
        .map(std::path::PathBuf::from)
        .and_then(|binary| {
            binary
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
        })
        .unwrap_or_else(unique_shim_id)
}

pub(crate) fn list_full_name(binary_id: &str, test_name: &str) -> String {
    format!("{binary_id}${test_name}")
}

fn should_use_nextest_env_for_instance(command: &[std::ffi::OsString]) -> bool {
    std::env::var("NEXTEST_TEST_PHASE").ok().as_deref() == Some("run")
        && command.first().is_some_and(|arg| {
            let path = arg.to_string_lossy();
            !path.ends_with(".sh") && !path.ends_with(".bat")
        })
}

fn should_use_nextest_env_for_list(command: &[std::ffi::OsString]) -> bool {
    std::env::var("NEXTEST_TEST_PHASE").ok().as_deref() == Some("list")
        && command.first().is_some_and(|arg| {
            let path = arg.to_string_lossy();
            !path.ends_with(".sh") && !path.ends_with(".bat")
        })
}

pub(crate) fn filesystem_safe_instance_id(full_name: &str) -> String {
    full_name.replace(['/', '\\'], "_")
}

fn exact_test_from_command(command: &[std::ffi::OsString]) -> Option<(std::path::PathBuf, String)> {
    if command.len() < 3 {
        return None;
    }
    let binary = std::path::PathBuf::from(&command[0]);
    let args: Vec<_> = command[1..]
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();
    let exact_index = args.iter().position(|arg| arg == "--exact")?;
    let test_name = args.get(exact_index + 1)?.clone();
    Some((binary, test_name))
}

fn unique_shim_id() -> String {
    kiss_publication_barrier::unique_process_suffix()
}
