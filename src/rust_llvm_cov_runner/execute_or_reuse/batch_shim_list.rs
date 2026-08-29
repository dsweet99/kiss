use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::Stdio;

use super::BatchShimListMetadata;
use super::batch_shim_child::build_delegated_command;
use super::batch_shim_write::{
    filesystem_safe_instance_id, list_binary_id, list_full_name, write_shim_list_metadata,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim_delegated::scrub_coverage_build_env;

pub(crate) fn run_delegated_list_child(
    output_dir: &Path,
    delegated: &[String],
    command: &[OsString],
) -> io::Result<i32> {
    let mut delegated_command = build_delegated_command(delegated, command);
    scrub_coverage_build_env(&mut delegated_command);

    let kiss_profraw = crate::rust_llvm_cov_runner::kiss_profraw::resolve_kiss_profraw(output_dir);
    crate::rust_llvm_cov_runner::kiss_profraw::ensure_kiss_profraw(&kiss_profraw)?;
    delegated_command.env(
        "LLVM_PROFILE_FILE",
        crate::rust_llvm_cov_runner::kiss_profraw::discard_llvm_profile_path(&kiss_profraw),
    );
    let child = delegated_command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let child_pid = child.id();
    let output = child.wait_with_output()?;
    let cleanup_err = crate::rust_llvm_cov_runner::kiss_profraw::cleanup_kiss_profraw_for_pid(
        &kiss_profraw,
        child_pid,
    )
    .err();
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    write_list_metadata(output_dir, command, &output.stdout)?;
    if let Some(err) = cleanup_err {
        return Err(err);
    }
    Ok(output.status.code().unwrap_or(1))
}

fn write_list_metadata(output_dir: &Path, command: &[OsString], stdout: &[u8]) -> io::Result<()> {
    let binary_id = list_binary_id(command);
    let mut test_names = discovered_test_names(stdout, &binary_id);
    test_names.sort();
    test_names.dedup();
    std::fs::create_dir_all(output_dir)?;
    let id = filesystem_safe_instance_id(&list_metadata_id(&binary_id, command));
    let metadata = BatchShimListMetadata {
        schema_version: super::SHIM_LIST_SCHEMA.to_string(),
        id: id.clone(),
        binary_id,
        argv: command
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect(),
        test_names,
    };
    write_shim_list_metadata(output_dir, &id, &metadata)
}

fn list_metadata_id(binary_id: &str, command: &[OsString]) -> String {
    let mut hash = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        b"list-id-v1",
    );
    for arg in command {
        hash = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
            hash,
            arg.to_string_lossy().as_bytes(),
        );
        hash = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(hash, &[0]);
    }
    format!("{binary_id}${hash:016x}")
}

fn discovered_test_names(stdout: &[u8], binary_id: &str) -> Vec<String> {
    stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_slice(line).ok()?;
            if value.get("type").and_then(serde_json::Value::as_str) != Some("test")
                || value.get("event").and_then(serde_json::Value::as_str) != Some("discovered")
            {
                return None;
            }
            value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .map(|name| list_full_name(binary_id, &name))
        .collect()
}
