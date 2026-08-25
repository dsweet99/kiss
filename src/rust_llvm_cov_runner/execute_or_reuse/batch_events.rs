use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde_json::Value;

use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::plan::batch_nextest_id::{
    libtest_binary_prefix, nextest_binary_id, package_name_from_manifest,
};

#[path = "batch_events_serde.rs"]
mod batch_events_serde;
use batch_events_serde::{CargoBuildFinished, CargoCompilerArtifact, LibtestRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchCompilerArtifact {
    pub executable: Option<String>,
    pub filenames: Vec<String>,
    pub nextest_binary_id: Option<String>,
    pub libtest_binary_prefix: Option<String>,
    pub src_path: Option<String>,
    pub is_test_harness: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatchTestTerminal {
    pub full_name: String,
    pub test_name: String,
    pub passed: bool,
    pub timed_out: bool,
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

        if line.first() != Some(&b'{') {
            continue;
        }
        if skip_unneeded_cargo_message(line) {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        ingest_event_line(&mut stream, &mut package_names, value, line_no + 1)?;
    }
    Ok(stream)
}

fn skip_unneeded_cargo_message(line: &[u8]) -> bool {
    if peeked_top_level_string_field(line, br#""type":""#).is_some() {
        return false;
    }
    match peeked_top_level_string_field(line, br#""reason":""#) {
        Some(b"compiler-artifact" | b"build-finished") => false,
        Some(_) => true,
        None => false,
    }
}

fn peeked_top_level_string_field<'a>(line: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let prefix = &line[..line.len().min(128)];
    let start = prefix.windows(key.len()).position(|window| window == key)?;
    let rest = &prefix[start + key.len()..];
    let end = rest.iter().position(|byte| *byte == b'"')?;
    Some(&rest[..end])
}

fn ingest_event_line(
    stream: &mut BatchEventStream,
    package_names: &mut BTreeMap<PathBuf, String>,
    value: Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    if value.get("type").is_some() {
        return ingest_libtest_event(stream, value, line_no);
    }
    if let Some(reason) = value.get("reason").and_then(Value::as_str) {
        return ingest_cargo_message(stream, package_names, reason.to_string(), value, line_no);
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "structured stdout line {line_no} is neither a Cargo message nor a libtest-json-plus record"
    )))
}

fn ingest_cargo_message(
    stream: &mut BatchEventStream,
    package_names: &mut BTreeMap<PathBuf, String>,
    reason: String,
    value: Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    match reason.as_str() {
        "compiler-artifact" => {
            let artifact = serde_json::from_value::<CargoCompilerArtifact>(value)
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
            let finished = serde_json::from_value::<CargoBuildFinished>(value)
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
    if artifact.executable.is_none()
        || artifact.manifest_path.is_empty()
        || artifact.target.name.is_empty()
    {
        return (None, None);
    }
    let Some(package_name) =
        package_name_from_manifest(std::path::Path::new(&artifact.manifest_path), package_names)
    else {
        return (None, None);
    };
    let nextest = nextest_binary_id(&package_name, &artifact.target.name, &artifact.target.kind);
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
    value: Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    if value.get("type").and_then(Value::as_str) != Some("test") {
        return Ok(());
    }
    let record = serde_json::from_value::<LibtestRecord>(value)
        .map_err(|err| json_shape_error(line_no, err))?;
    apply_libtest_record(stream, record, line_no)
}

fn apply_libtest_record(
    stream: &mut BatchEventStream,
    record: LibtestRecord,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    let Some((full_name, test_name)) = split_libtest_name(&record.name) else {
        return skip_nameless_libtest_event(&record.event, line_no);
    };
    match record.event.as_str() {
        "discovered" => stream.discovered_tests.push(BatchTestStarted {
            full_name,
            test_name,
        }),
        "started" => stream.started_tests.push(BatchTestStarted {
            full_name,
            test_name,
        }),
        "ok" | "failed" | "timeout" | "timed_out" => {
            let timed_out = record.event == "timeout"
                || record.event == "timed_out"
                || record
                    .reason
                    .as_deref()
                    .is_some_and(is_nextest_timeout_reason);
            stream.terminal_tests.push(BatchTestTerminal {
                full_name,
                test_name,
                passed: record.event == "ok",
                timed_out,
                exec_time_secs: record.exec_time.unwrap_or(0.0),
                stdout: record.stdout,
                reason: record.reason,
            });
        }
        "ignored" => stream.ignored_tests.push(BatchTestStarted {
            full_name,
            test_name,
        }),
        other => {
            return Err(unsupported_libtest_event(line_no, other));
        }
    }
    Ok(())
}

fn skip_nameless_libtest_event(event: &str, line_no: usize) -> Result<(), RustLlvmCovError> {
    match event {
        "discovered" | "started" | "ok" | "failed" | "timeout" | "timed_out" | "ignored" => Ok(()),
        other => Err(unsupported_libtest_event(line_no, other)),
    }
}

fn unsupported_libtest_event(line_no: usize, event: &str) -> RustLlvmCovError {
    RustLlvmCovError::InvalidRequest(format!(
        "structured stdout line {line_no} has unsupported libtest event `{event}`"
    ))
}

fn is_nextest_timeout_reason(reason: &str) -> bool {
    let lowered = reason.to_ascii_lowercase();
    lowered.contains("time limit exceeded") || lowered.contains("timed out")
}

fn split_libtest_name(name: &str) -> Option<(String, String)> {
    let (_prefix, test_name) = name.rsplit_once('$')?;
    Some((name.to_string(), test_name.to_string()))
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

pub struct SelectorMatchIndex<'a> {
    indexed: bool,
    names: HashSet<&'a str>,
    selectors: &'a [String],
    exact: bool,
}

impl<'a> SelectorMatchIndex<'a> {
    pub fn new(selectors: &'a [String], exact: bool) -> Self {
        let indexed = exact || selectors.len() > 64;
        Self {
            indexed,
            names: if indexed {
                selectors.iter().map(String::as_str).collect()
            } else {
                HashSet::new()
            },
            selectors,
            exact,
        }
    }

    pub fn matches(&self, full_name: &str) -> bool {
        if self.indexed {
            indexed_name_tokens(full_name).any(|token| self.names.contains(token))
        } else {
            self.selectors
                .iter()
                .any(|selector| selector_matches_test(full_name, selector, self.exact))
        }
    }

    pub fn matching_selectors(&self, full_name: &str) -> Vec<String> {
        if self.indexed {
            let mut out = Vec::new();
            for token in indexed_name_tokens(full_name) {
                if self.names.contains(token) && !out.iter().any(|existing| existing == token) {
                    out.push(token.to_string());
                }
            }
            out
        } else {
            aggregate_selectors_for_test(full_name, self.selectors, self.exact)
        }
    }
}

fn indexed_name_tokens(full_name: &str) -> impl Iterator<Item = &str> {
    let suffix = full_name
        .rsplit_once('$')
        .map(|(_, test)| test)
        .unwrap_or(full_name);
    std::iter::once(full_name)
        .chain(std::iter::once(suffix))
        .chain(suffix_after_each_colon_colon(suffix))
}

fn suffix_after_each_colon_colon(name: &str) -> impl Iterator<Item = &str> {
    name.match_indices("::")
        .map(|(index, _)| &name[index + 2..])
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
