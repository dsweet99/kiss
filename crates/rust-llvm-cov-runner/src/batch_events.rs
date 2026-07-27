use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::batch_nextest_id::{
    libtest_binary_prefix, nextest_binary_id, package_name_from_manifest,
};
use crate::RustLlvmCovError;

#[path = "batch_events_serde.rs"]
mod batch_events_serde;
use batch_events_serde::{CargoBuildFinished, CargoCompilerArtifact, LibtestRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchCompilerArtifact {
    pub executable: Option<String>,
    pub filenames: Vec<String>,
    /// Nextest / libtest-json-plus binary id when this artifact is a runnable test harness.
    pub nextest_binary_id: Option<String>,
    /// Libtest-json-plus name prefix (`{package}::{target}`) for unit-test harnesses.
    pub libtest_binary_prefix: Option<String>,
    /// `target.src_path` from the cargo compiler-artifact (crate root for the target).
    pub src_path: Option<String>,
    /// True when cargo reported `profile.test` (unit/integration test harness).
    pub is_test_harness: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchTestTerminal {
    pub full_name: String,
    pub test_name: String,
    pub passed: bool,
    pub exec_time_secs: f64,
    pub stdout: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchTestStarted {
    pub full_name: String,
    pub test_name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatchEventStream {
    pub build_succeeded: Option<bool>,
    pub compiler_artifacts: Vec<BatchCompilerArtifact>,
    pub discovered_tests: Vec<BatchTestStarted>,
    pub started_tests: Vec<BatchTestStarted>,
    pub ignored_tests: Vec<BatchTestStarted>,
    pub terminal_tests: Vec<BatchTestTerminal>,
}

pub fn parse_batch_event_stream(stdout: &[u8]) -> Result<BatchEventStream, RustLlvmCovError> {
    let mut stream = BatchEventStream::default();
    let mut package_names = BTreeMap::new();
    for (line_no, line) in stdout.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_slice(line).map_err(|err| {
            RustLlvmCovError::InvalidRequest(format!(
                "structured stdout line {} is not valid JSON: {err}",
                line_no + 1
            ))
        })?;
        ingest_event_line(&mut stream, &mut package_names, &value, line_no + 1)?;
    }
    Ok(stream)
}

fn ingest_event_line(
    stream: &mut BatchEventStream,
    package_names: &mut BTreeMap<PathBuf, String>,
    value: &Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    if value.get("type").is_some() {
        return ingest_libtest_event(stream, value, line_no);
    }
    if let Some(reason) = value.get("reason").and_then(Value::as_str) {
        return ingest_cargo_message(stream, package_names, reason, value, line_no);
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "structured stdout line {line_no} is neither a Cargo message nor a libtest-json-plus record"
    )))
}

fn ingest_cargo_message(
    stream: &mut BatchEventStream,
    package_names: &mut BTreeMap<PathBuf, String>,
    reason: &str,
    value: &Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    match reason {
        "compiler-artifact" => {
            let artifact = serde_json::from_value::<CargoCompilerArtifact>(value.clone())
                .map_err(|err| json_shape_error(line_no, err))?;
            let (nextest_binary_id, libtest_binary_prefix) =
                artifact_binary_ids(&artifact, package_names);
            let src_path = non_empty_string(artifact.target.src_path);
            let is_test_harness = artifact.profile.test;
            stream.compiler_artifacts.push(BatchCompilerArtifact {
                executable: artifact.executable,
                filenames: artifact.filenames,
                nextest_binary_id,
                libtest_binary_prefix,
                src_path,
                is_test_harness,
            });
        }
        "build-finished" => {
            let finished = serde_json::from_value::<CargoBuildFinished>(value.clone())
                .map_err(|err| json_shape_error(line_no, err))?;
            stream.build_succeeded = Some(finished.success);
        }
        "build-script-executed" => {}
        _ => {}
    }
    Ok(())
}

fn artifact_binary_ids(
    artifact: &CargoCompilerArtifact,
    package_names: &mut BTreeMap<PathBuf, String>,
) -> (Option<String>, Option<String>) {
    if artifact.executable.is_none() || artifact.manifest_path.is_empty() || artifact.target.name.is_empty()
    {
        return (None, None);
    }
    let Some(package_name) =
        package_name_from_manifest(std::path::Path::new(&artifact.manifest_path), package_names)
    else {
        return (None, None);
    };
    let nextest = nextest_binary_id(
        &package_name,
        &artifact.target.name,
        &artifact.target.kind,
    );
    let libtest = libtest_binary_prefix(&package_name, &artifact.target.name);
    (Some(nextest), Some(libtest))
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn ingest_libtest_event(
    stream: &mut BatchEventStream,
    value: &Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    let Some(record_type) = value.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    if record_type != "test" {
        return Ok(());
    }
    let record = serde_json::from_value::<LibtestRecord>(value.clone())
        .map_err(|err| json_shape_error(line_no, err))?;
    match record.event.as_str() {
        "discovered" => {
            let (full_name, test_name) = split_libtest_name(&record.name, line_no)?;
            stream.discovered_tests.push(BatchTestStarted {
                full_name,
                test_name,
            });
        }
        "started" => {
            let (full_name, test_name) = split_libtest_name(&record.name, line_no)?;
            stream.started_tests.push(BatchTestStarted {
                full_name,
                test_name,
            });
        }
        "ok" | "failed" => {
            let (full_name, test_name) = split_libtest_name(&record.name, line_no)?;
            stream.terminal_tests.push(BatchTestTerminal {
                full_name,
                test_name,
                passed: record.event == "ok",
                exec_time_secs: record.exec_time.unwrap_or(0.0),
                stdout: record.stdout,
                reason: record.reason,
            });
        }
        "ignored" => {
            let (full_name, test_name) = split_libtest_name(&record.name, line_no)?;
            stream.ignored_tests.push(BatchTestStarted {
                full_name,
                test_name,
            });
        }
        other => {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "structured stdout line {line_no} has unsupported libtest event `{other}`"
            )));
        }
    }
    Ok(())
}

fn split_libtest_name(name: &str, line_no: usize) -> Result<(String, String), RustLlvmCovError> {
    let Some((_prefix, test_name)) = name.rsplit_once('$') else {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "structured stdout line {line_no} libtest name `{name}` is missing `$` test suffix"
        )));
    };
    Ok((name.to_string(), test_name.to_string()))
}

fn json_shape_error(line_no: usize, err: serde_json::Error) -> RustLlvmCovError {
    RustLlvmCovError::InvalidRequest(format!(
        "structured stdout line {line_no} has unexpected JSON shape: {err}"
    ))
}

pub fn selector_matches_test(full_name: &str, selector: &str, exact: bool) -> bool {
    if exact {
        full_name == selector
            || full_name
                .rsplit_once('$')
                .is_some_and(|(_, test)| test == selector)
    } else {
        full_name.contains(selector)
    }
}

pub fn aggregate_selectors_for_test(
    full_name: &str,
    selectors: &[String],
    exact: bool,
) -> Vec<String> {
    selectors
        .iter()
        .filter(|selector| selector_matches_test(full_name, selector, exact))
        .cloned()
        .collect()
}

pub fn rust_test_args_include_ignored(test_args: &[String]) -> bool {
    test_args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--ignored" | "--include-ignored"))
}

#[cfg(test)]
#[path = "batch_events_test.rs"]
mod tests;
