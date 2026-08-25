use super::*;

fn sample_stream_bytes() -> Vec<u8> {
    br#"{"reason":"compiler-artifact","executable":"/tmp/bin","filenames":["/tmp/bin"],"fresh":false}
{"reason":"build-finished","success":true}
{"type":"test","event":"discovered","name":"pkg::bin$alpha_case"}
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
    assert_eq!(parsed.discovered_tests.len(), 1);
    assert_eq!(parsed.started_tests.len(), 1);
    assert_eq!(parsed.started_tests[0].full_name, "pkg::bin$alpha_case");
    assert_eq!(parsed.terminal_tests.len(), 2);
    assert!(parsed.terminal_tests[0].passed);
    assert!(!parsed.terminal_tests[0].timed_out);
    assert!(!parsed.terminal_tests[1].passed);
    assert!(!parsed.terminal_tests[1].timed_out);
    assert_eq!(parsed.terminal_tests[1].stdout.as_deref(), Some("boom"));
}

#[test]
fn parser_maps_time_limit_exceeded_reason_to_timed_out() {
    let bytes = br#"{"type":"test","event":"failed","name":"pkg::bin$slow","exec_time":1.0,"reason":"time limit exceeded"}
"#;
    let parsed = parse_batch_event_stream(bytes).unwrap();
    assert_eq!(parsed.terminal_tests.len(), 1);
    assert!(!parsed.terminal_tests[0].passed);
    assert!(parsed.terminal_tests[0].timed_out);
}

#[test]
fn parser_ignores_non_json_noise_lines() {
    let parsed = parse_batch_event_stream(b"not json\nchild-out\n{\n").unwrap();
    assert!(parsed.terminal_tests.is_empty());
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
fn large_selector_index_matches_suffix_without_substring() {
    let selectors: Vec<String> = (0..80).map(|i| format!("t{i}")).collect();
    let index = SelectorMatchIndex::new(&selectors, false);
    assert!(index.matches("pkg::bin$t12"));
    assert!(index.matches("kiss::kiss$mod::nested::t12"));
    assert!(!index.matches("pkg::bin$t12_extra"));
    assert_eq!(
        index.matching_selectors("kiss::kiss$mod::nested::t12"),
        vec!["t12".to_string()]
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
fn parser_ignores_nested_libtest_names_without_dollar_suffix() {
    let parsed =
        parse_batch_event_stream(br#"{"type":"test","event":"ok","name":"alpha"}"#).unwrap();
    assert!(parsed.terminal_tests.is_empty());
}

#[test]
fn parser_rejects_unsupported_libtest_events() {
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
fn parser_skips_compiler_message_payloads_without_full_parse() {
    let mut stdout = b"{\"reason\":\"compiler-message\",\"message\":\"".to_vec();
    stdout.extend(std::iter::repeat_n(b'x', 64_000));
    stdout.extend_from_slice(br#""}
{"reason":"build-finished","success":true}
{"type":"test","event":"ok","name":"pkg::bin$alpha","exec_time":0.001,"reason":"time limit exceeded"}
"#);
    let parsed = parse_batch_event_stream(&stdout).unwrap();
    assert_eq!(parsed.build_succeeded, Some(true));
    assert_eq!(parsed.terminal_tests.len(), 1);
    assert!(parsed.terminal_tests[0].timed_out);
}

#[test]
fn batch_event_types_are_constructible() {
    let artifact = BatchCompilerArtifact {
        executable: Some("/tmp/bin".to_string()),
        filenames: vec!["/tmp/a.o".to_string()],
        nextest_binary_id: None,
        libtest_binary_prefix: None,
        src_path: None,
        is_test_harness: false,
    };
    let terminal = BatchTestTerminal {
        full_name: "pkg::bin$alpha".to_string(),
        test_name: "alpha".to_string(),
        passed: true,
        timed_out: false,
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
    use super::batch_events_serde::{
        CargoBuildFinished, CargoCompilerArtifact, CargoProfile, CargoTarget, LibtestRecord,
    };
    use serde_json::json;

    let artifact = CargoCompilerArtifact {
        executable: Some("/tmp/bin".into()),
        filenames: vec!["/tmp/a.o".into()],
        manifest_path: String::new(),
        target: CargoTarget::default(),
        profile: CargoProfile::default(),
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
