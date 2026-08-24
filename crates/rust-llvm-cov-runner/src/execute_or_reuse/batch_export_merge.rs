use std::path::{Path, PathBuf};
use std::process::Command;

use crate::execute_or_reuse::batch_export_tools::ExportTools;
use crate::execute_or_reuse::llvm_cov_json::parse_llvm_cov_json;
use crate::{RustLineCoverage, RustLlvmCovError};

pub(crate) fn merge_profiles(
    tools: &ExportTools,
    profile_inputs: &[PathBuf],
    profdata_output: &Path,
) -> Result<(), RustLlvmCovError> {
    if profile_inputs.is_empty() {
        return Err(RustLlvmCovError::InvalidRequest(
            "profile merge requires at least one input".into(),
        ));
    }
    let status = Command::new(&tools.llvm_profdata)
        .arg("merge")
        .arg("-sparse")
        .arg("--num-threads=1")
        .args(profile_inputs)
        .arg("-o")
        .arg(profdata_output)
        .status()
        .map_err(RustLlvmCovError::Io)?;
    if !status.success() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "llvm-profdata merge failed for {} profile input(s)",
            profile_inputs.len()
        )));
    }
    Ok(())
}

pub(crate) fn merge_instance_profile(
    tools: &ExportTools,
    profile_input: &Path,
    profdata_output: &Path,
) -> Result<bool, RustLlvmCovError> {
    if !profile_input.is_file() {
        return Ok(false);
    }
    let output = Command::new(&tools.llvm_profdata)
        .arg("merge")
        .arg("-sparse")
        .arg("--num-threads=1")
        .arg(profile_input)
        .arg("-o")
        .arg(profdata_output)
        .output()
        .map_err(RustLlvmCovError::Io)?;
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if unusable_profile_stderr(&stderr) {
        return Ok(false);
    }
    Err(RustLlvmCovError::InvalidRequest(
        "llvm-profdata merge failed for 1 profile input(s)".into(),
    ))
}

pub(crate) fn unusable_profile_stderr(stderr: &str) -> bool {
    stderr.contains("no profile can be merged")
        || stderr.contains("malformed instrumentation profile data")
        || stderr.contains("truncated profile data")
}

pub(crate) fn export_instance_coverage(
    tools: &ExportTools,
    profdata: &Path,
    source_root: &Path,
    objects: &[PathBuf],
    ignore_filename_regex: Option<&str>,
) -> Result<RustLineCoverage, RustLlvmCovError> {
    let mut command = Command::new(&tools.llvm_cov);
    command
        .arg("export")
        .arg("-format=text")
        .arg("--threads=1")
        .arg("-skip-expansions")
        .arg("-skip-functions")
        .arg("-instr-profile")
        .arg(profdata);
    if let Some(regex) = ignore_filename_regex {
        command.arg("-ignore-filename-regex").arg(regex);
    }
    for object in objects {
        command.arg("-object").arg(object);
    }
    let output = command.output().map_err(RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "llvm-cov export failed for {}: {}",
            profdata.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    parse_llvm_cov_json(&output.stdout, source_root)
}

#[cfg(test)]
#[path = "batch_export_unusable_test.rs"]
mod unusable_tests;
