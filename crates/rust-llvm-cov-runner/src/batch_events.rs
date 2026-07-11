use serde::Deserialize;
use serde_json::Value;

use crate::RustLlvmCovError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchCompilerArtifact {
    pub executable: Option<String>,
    pub filenames: Vec<String>,
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
    pub started_tests: Vec<BatchTestStarted>,
    pub ignored_tests: Vec<BatchTestStarted>,
    pub terminal_tests: Vec<BatchTestTerminal>,
}

pub fn parse_batch_event_stream(stdout: &[u8]) -> Result<BatchEventStream, RustLlvmCovError> {
    let mut stream = BatchEventStream::default();
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
        ingest_event_line(&mut stream, &value, line_no + 1)?;
    }
    Ok(stream)
}

fn ingest_event_line(
    stream: &mut BatchEventStream,
    value: &Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    if value.get("type").is_some() {
        return ingest_libtest_event(stream, value, line_no);
    }
    if let Some(reason) = value.get("reason").and_then(Value::as_str) {
        return ingest_cargo_message(stream, reason, value, line_no);
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "structured stdout line {line_no} is neither a Cargo message nor a libtest-json-plus record"
    )))
}

fn ingest_cargo_message(
    stream: &mut BatchEventStream,
    reason: &str,
    value: &Value,
    line_no: usize,
) -> Result<(), RustLlvmCovError> {
    match reason {
        "compiler-artifact" => {
            let artifact = serde_json::from_value::<CargoCompilerArtifact>(value.clone())
                .map_err(|err| json_shape_error(line_no, err))?;
            stream.compiler_artifacts.push(BatchCompilerArtifact {
                executable: artifact.executable,
                filenames: artifact.filenames,
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

#[derive(Deserialize)]
struct CargoCompilerArtifact {
    executable: Option<String>,
    #[serde(default)]
    filenames: Vec<String>,
}

#[derive(Deserialize)]
struct CargoBuildFinished {
    success: bool,
}

#[derive(Deserialize)]
struct LibtestRecord {
    event: String,
    name: String,
    exec_time: Option<f64>,
    stdout: Option<String>,
    reason: Option<String>,
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
mod tests {
    use super::*;

    fn sample_stream_bytes() -> Vec<u8> {
        br#"{"reason":"compiler-artifact","executable":"/tmp/bin","filenames":["/tmp/bin"],"fresh":false}
{"reason":"build-finished","success":true}
{"type":"test","event":"started","name":"pkg::bin$alpha_case"}
{"type":"test","event":"ok","name":"pkg::bin$alpha_case","exec_time":0.002}
{"type":"test","event":"failed","name":"pkg::bin$beta_case","exec_time":0.003,"stdout":"boom"}
{"type":"suite","event":"failed","passed":1,"failed":1,"ignored":0,"measured":0,"filtered_out":0,"exec_time":0.005}
"#
        .to_vec()
    }

    #[test]
    fn parser_collects_build_artifacts_and_terminal_tests() {
        let parsed = parse_batch_event_stream(&sample_stream_bytes()).unwrap();

        assert_eq!(parsed.build_succeeded, Some(true));
        assert_eq!(parsed.compiler_artifacts.len(), 1);
        assert_eq!(
            parsed.compiler_artifacts[0].executable.as_deref(),
            Some("/tmp/bin")
        );
        assert_eq!(parsed.started_tests.len(), 1);
        assert_eq!(parsed.started_tests[0].full_name, "pkg::bin$alpha_case");
        assert_eq!(parsed.terminal_tests.len(), 2);
        assert!(parsed.terminal_tests[0].passed);
        assert!(!parsed.terminal_tests[1].passed);
        assert_eq!(parsed.terminal_tests[1].stdout.as_deref(), Some("boom"));
    }

    #[test]
    fn parser_rejects_non_json_lines() {
        let err = parse_batch_event_stream(b"not json\n").unwrap_err();
        assert!(matches!(err, RustLlvmCovError::InvalidRequest(_)));
    }

    #[test]
    fn selector_matching_shares_substring_and_exact_semantics() {
        let full = "pkg::bin$alpha_beta";
        assert!(selector_matches_test(full, "alpha", false));
        assert!(!selector_matches_test(full, "alpha", true));
        assert!(selector_matches_test(full, "alpha_beta", true));
        assert_eq!(
            aggregate_selectors_for_test(full, &["alpha".to_string(), "beta".to_string()], false),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn parser_records_ignored_tests_without_terminal_events() {
        let stdout = br#"{"type":"test","event":"started","name":"pkg::bin$skip"}
{"type":"test","event":"ignored","name":"pkg::bin$skip"}
"#
        .as_slice();
        let parsed = parse_batch_event_stream(stdout).unwrap();
        assert_eq!(parsed.started_tests.len(), 1);
        assert_eq!(parsed.ignored_tests.len(), 1);
        assert_eq!(parsed.ignored_tests[0].full_name, "pkg::bin$skip");
        assert!(parsed.terminal_tests.is_empty());
    }

    #[test]
    fn parser_ignores_build_script_and_non_test_libtest_records() {
        let stdout = br#"{"reason":"build-script-executed","linked_libs":[],"linked_paths":[],"cfgs":[],"env":[],"out_dir":"/tmp/out"}
{"type":"test","event":"ignored","name":"pkg::bin$skip"}
{"type":"bench","event":"ok","name":"pkg::bin$bench"}
"#.as_slice();
        let parsed = parse_batch_event_stream(stdout).unwrap();
        assert!(parsed.terminal_tests.is_empty());
        assert!(parsed.started_tests.is_empty());
        assert_eq!(parsed.ignored_tests.len(), 1);
        assert!(parsed.build_succeeded.is_none());
    }

    #[test]
    fn parser_rejects_missing_dollar_suffix_and_unsupported_events() {
        let missing_suffix =
            parse_batch_event_stream(br#"{"type":"test","event":"ok","name":"badname"}"#)
                .unwrap_err();
        assert!(matches!(
            missing_suffix,
            RustLlvmCovError::InvalidRequest(_)
        ));

        let unsupported =
            parse_batch_event_stream(br#"{"type":"test","event":"unknown","name":"pkg::bin$x"}"#)
                .unwrap_err();
        assert!(matches!(unsupported, RustLlvmCovError::InvalidRequest(_)));
    }

    #[test]
    fn parser_rejects_unknown_top_level_records() {
        let err = parse_batch_event_stream(br#"{"foo":"bar"}"#).unwrap_err();
        assert!(matches!(err, RustLlvmCovError::InvalidRequest(_)));
    }

    #[test]
    fn parser_rejects_malformed_cargo_artifact_shape() {
        let err = parse_batch_event_stream(br#"{"reason":"build-finished"}"#).unwrap_err();
        assert!(matches!(err, RustLlvmCovError::InvalidRequest(_)));
        let err = parse_batch_event_stream(br#"{"type":"test","event":"ok"}"#).unwrap_err();
        assert!(matches!(err, RustLlvmCovError::InvalidRequest(_)));
    }

    #[test]
    fn parser_records_failed_build_marker() {
        let parsed =
            parse_batch_event_stream(br#"{"reason":"build-finished","success":false}"#).unwrap();
        assert_eq!(parsed.build_succeeded, Some(false));
    }

    #[test]
    fn batch_event_types_are_constructible() {
        let artifact = BatchCompilerArtifact {
            executable: Some("/tmp/bin".to_string()),
            filenames: vec!["/tmp/a.o".to_string()],
        };
        let terminal = BatchTestTerminal {
            full_name: "pkg::bin$alpha".to_string(),
            test_name: "alpha".to_string(),
            passed: true,
            exec_time_secs: 0.1,
            stdout: None,
            reason: None,
        };
        let started = BatchTestStarted {
            full_name: "pkg::bin$alpha".to_string(),
            test_name: "alpha".to_string(),
        };
        assert_eq!(artifact.filenames.len(), 1);
        assert_eq!(terminal.test_name, "alpha");
        assert_eq!(started.test_name, "alpha");
    }

    #[test]
    fn private_deserialize_record_types_round_trip() {
        use serde_json::json;

        let artifact = CargoCompilerArtifact {
            executable: Some("/tmp/bin".into()),
            filenames: vec!["/tmp/a.o".into()],
        };
        assert_eq!(artifact.executable.as_deref(), Some("/tmp/bin"));
        let decoded: CargoCompilerArtifact = serde_json::from_value(json!({
            "executable": "/tmp/bin",
            "filenames": ["/tmp/a.o"]
        }))
        .unwrap();
        assert_eq!(decoded.filenames, artifact.filenames);

        let finished = CargoBuildFinished { success: false };
        assert!(!finished.success);
        let decoded_finished: CargoBuildFinished =
            serde_json::from_value(json!({"success": false})).unwrap();
        assert_eq!(decoded_finished.success, finished.success);

        let record = LibtestRecord {
            event: "failed".to_string(),
            name: "pkg::bin$case".to_string(),
            exec_time: None,
            stdout: None,
            reason: Some("assertion failed".to_string()),
        };
        assert_eq!(record.reason.as_deref(), Some("assertion failed"));
        let decoded_record: LibtestRecord = serde_json::from_value(json!({
            "event": "failed",
            "name": "pkg::bin$case",
            "reason": "assertion failed"
        }))
        .unwrap();
        assert_eq!(decoded_record.event, record.event);
    }
}
