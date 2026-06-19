use super::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn parsed_rs(path: PathBuf) -> ParsedRustFile {
    let source = "pub fn covered() -> usize { 1 }\npub fn missed() -> usize { 2 }\n";
    let ast = syn::parse_file(source).unwrap();
    ParsedRustFile {
        path,
        source: source.to_string(),
        ast,
    }
}

#[test]
fn parse_llvm_cov_json_extracts_line_level_file_coverage() {
    let repo = Path::new("/repo");
    let payload = serde_json::json!({
        "data": [{
            "files": [
                {
                    "filename": "/repo/src/lib.rs",
                    "segments": [
                        [1, 1, 3, true, true, false],
                        [2, 1, 0, true, true, false],
                        [3, 1, 0, false, false, false]
                    ],
                    "summary": {"lines": {"percent": 50.0}}
                },
                {
                    "filename": "/repo/tests/basic.rs",
                    "segments": [[1, 1, 1, true, true, false]]
                },
                {
                    "filename": null,
                    "segments": [[1, 1, 1, true, true, false]]
                }
            ]
        }]
    });

    let got = parse_llvm_cov_json(repo, &payload.to_string()).unwrap();

    assert_eq!(
        got,
        vec![RustLineCoverage {
            file: PathBuf::from("src/lib.rs"),
            executable_lines: vec![1, 2],
            missing_lines: vec![2],
        }]
    );
}

#[test]
fn parse_llvm_cov_json_ignores_malformed_segments_and_empty_files() {
    let repo = Path::new("/repo");
    let payload = serde_json::json!({
        "data": [{
            "files": [
                {
                    "filename": "/repo/src/lib.rs",
                    "segments": [
                        "not a segment",
                        [null, 1, 1, true, true, false],
                        [2, 1, null, true, true, false],
                        [3, 1, 0, false, true, false],
                        [4, 1, 1, true, true, false]
                    ]
                },
                {
                    "filename": "/repo/src/missing_segments.rs"
                },
                {
                    "filename": "/repo/src/empty.rs",
                    "segments": [[8, 1, 0, false, true, false]]
                }
            ]
        }]
    });

    let got = parse_llvm_cov_json(repo, &payload.to_string()).unwrap();

    assert_eq!(
        got,
        vec![RustLineCoverage {
            file: PathBuf::from("src/lib.rs"),
            executable_lines: vec![4],
            missing_lines: vec![],
        }]
    );
}

#[test]
fn parse_llvm_cov_json_rejects_invalid_json() {
    let err = parse_llvm_cov_json(Path::new("/repo"), "{not json").unwrap_err();

    assert!(!err.is_empty());
}

#[test]
fn analysis_from_line_coverage_maps_lines_to_coverage_defs() {
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];
    let coverage = vec![RustLineCoverage {
        file: PathBuf::from("src/lib.rs"),
        executable_lines: vec![1, 2],
        missing_lines: vec![2],
    }];

    let analysis = analysis_from_line_coverage(&parsed, &coverage);

    let defs: Vec<_> = analysis
        .definitions
        .iter()
        .map(|d| (&d.name, d.line))
        .collect();
    let missing: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.name, d.line))
        .collect();
    assert_eq!(
        defs,
        vec![(&"line_1".to_string(), 1), (&"line_2".to_string(), 2)]
    );
    assert_eq!(missing, vec![(&"line_2".to_string(), 2)]);
}

#[test]
fn analysis_from_line_coverage_matches_absolute_parsed_paths() {
    let parsed_path = PathBuf::from("/repo/src/lib.rs");
    let parsed = vec![parsed_rs(parsed_path.clone())];
    let coverage = vec![RustLineCoverage {
        file: PathBuf::from("src/lib.rs"),
        executable_lines: vec![1, 2],
        missing_lines: vec![2],
    }];

    let analysis = analysis_from_line_coverage(&parsed, &coverage);

    assert_eq!(analysis.definitions.len(), 2);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert!(
        analysis
            .definitions
            .iter()
            .all(|def| def.file == parsed_path)
    );
    assert_eq!(analysis.unreferenced[0].line, 2);
}

#[test]
fn analysis_from_line_coverage_fails_closed_without_runtime_executable_lines() {
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = analysis_from_line_coverage(&parsed, &[]);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "llvm_cov_missing");
    assert_eq!(analysis.unreferenced[0].file, PathBuf::from("src/lib.rs"));
}

#[test]
fn fail_closed_runtime_analysis_marks_all_parsed_files_uncovered() {
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = fail_closed_runtime_analysis(&parsed);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "llvm_cov_failed");
}

#[test]
fn fail_closed_runtime_analysis_preserves_all_input_files_and_empty_sets() {
    let parsed = vec![
        parsed_rs(PathBuf::from("src/lib.rs")),
        parsed_rs(PathBuf::from("src/main.rs")),
    ];

    let analysis = fail_closed_runtime_analysis(&parsed);

    let files: Vec<_> = analysis
        .definitions
        .iter()
        .map(|def| def.file.clone())
        .collect();
    assert_eq!(
        files,
        vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")]
    );
    assert_eq!(analysis.unreferenced.len(), analysis.definitions.len());
    assert!(analysis.test_references.is_empty());
    assert!(analysis.call_references.is_empty());
    assert!(analysis.propagated_references.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn nested_cargo_llvm_cov_analysis_fails_closed_when_recursive_skip_is_disabled() {
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = nested_cargo_llvm_cov_analysis(&parsed, false);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "llvm_cov_failed");
}

#[test]
fn nested_cargo_llvm_cov_analysis_can_skip_recursive_test_binary_probe() {
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = nested_cargo_llvm_cov_analysis(&parsed, true);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn runtime_rust_analysis_skips_nested_cargo_llvm_cov_run_in_test_binary() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("CARGO_LLVM_COV", "1") };
    let tmp = tempfile::TempDir::new().unwrap();
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = runtime_rust_analysis(tmp.path(), &parsed);

    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn runtime_rust_analysis_skips_nested_target_dir_coverage_run_in_test_binary() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("CARGO_LLVM_COV_TARGET_DIR", "/tmp/kiss-nested-cov") };
    let tmp = tempfile::TempDir::new().unwrap();
    let parsed = vec![parsed_rs(PathBuf::from("src/lib.rs"))];

    let analysis = runtime_rust_analysis(tmp.path(), &parsed);

    unsafe { std::env::remove_var("CARGO_LLVM_COV_TARGET_DIR") };
    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn runtime_rust_analysis_empty_input_does_not_require_cargo_project() {
    let tmp = tempfile::TempDir::new().unwrap();

    let analysis = runtime_rust_analysis(tmp.path(), &[]);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
}

#[test]
fn cargo_llvm_cov_command_uses_json_output_at_repo_root() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = Path::new("/tmp/cov.json");

    let cmd = cargo_llvm_cov_command(tmp.path(), out);

    assert_eq!(cmd.cwd, tmp.path());
    assert_eq!(
        cmd.args,
        vec![
            "llvm-cov",
            "--workspace",
            "--json",
            "--output-path",
            "/tmp/cov.json"
        ]
    );
    assert_eq!(
        cmd.env,
        vec![("RUST_TEST_THREADS".to_string(), "1".to_string())]
    );
}

#[test]
fn cargo_llvm_cov_command_uses_nextest_when_gate_runner_requires_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join(".malvin")).unwrap();
    std::fs::write(tmp.path().join(".malvin/checks"), "cargo nextest run\n").unwrap();
    let out = Path::new("/tmp/cov.json");

    let cmd = cargo_llvm_cov_command(tmp.path(), out);

    assert_eq!(
        cmd.args,
        vec![
            "llvm-cov",
            "nextest",
            "--workspace",
            "--json",
            "--output-path",
            "/tmp/cov.json"
        ]
    );
}

#[test]
fn cargo_llvm_cov_command_uses_nextest_config_files() {
    for rel in ["nextest.toml", ".config/nextest.toml"] {
        let tmp = tempfile::TempDir::new().unwrap();
        if let Some(parent) = Path::new(rel).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir(tmp.path().join(parent)).unwrap();
        }
        std::fs::write(tmp.path().join(rel), "[profile.default]\n").unwrap();

        let cmd = cargo_llvm_cov_command(tmp.path(), Path::new("/tmp/cov.json"));

        assert_eq!(cmd.args[0], "llvm-cov");
        assert_eq!(cmd.args[1], "nextest");
    }
}

#[test]
fn nested_coverage_env_removes_workspace_wrapper() {
    let keys = rust_coverage_env_keys_to_remove();
    assert!(keys.contains(&"RUSTC_WRAPPER"));
    assert!(keys.contains(&"RUSTC_WORKSPACE_WRAPPER"));
}

#[test]
fn backend_fingerprint_changes_when_cargo_manifest_changes() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname='a'\n").unwrap();
    let first = backend_fingerprint(tmp.path());

    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname='b'\n").unwrap();
    let second = backend_fingerprint(tmp.path());

    assert_ne!(first, second);
}

#[test]
fn backend_fingerprint_changes_when_coverage_env_changes() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname='a'\n").unwrap();
    unsafe { std::env::remove_var("CARGO_TARGET_DIR") };
    let first = backend_fingerprint(tmp.path());

    unsafe { std::env::set_var("CARGO_TARGET_DIR", tmp.path().join("target-a")) };
    let second = backend_fingerprint(tmp.path());
    unsafe { std::env::remove_var("CARGO_TARGET_DIR") };

    assert_ne!(first, second);
}

#[test]
fn backend_fingerprint_includes_nextest_tool_when_required() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("nextest.toml"), "[profile.default]\n").unwrap();

    let fingerprint = backend_fingerprint(tmp.path());

    assert_eq!(fingerprint.len(), 16);
    assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn command_version_reports_empty_output_status_and_spawn_errors() {
    let empty = command_version("true", &[]);
    let missing = command_version("__kiss_missing_command_for_version_test__", &[]);

    assert!(empty.starts_with("status:"));
    assert!(missing.starts_with("ERROR:"));
}

#[test]
fn temp_output_path_is_process_scoped_json_path() {
    let path = temp_output_path();
    let text = path.to_string_lossy();

    assert!(text.contains(&format!("kiss-llvm-cov-{}", std::process::id())));
    assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("json"));
}

#[test]
fn analysis_from_line_coverage_fails_closed_on_ambiguous_suffix_matches() {
    let parsed = vec![parsed_rs(PathBuf::from("/repo/src/lib.rs"))];
    let coverage = vec![
        RustLineCoverage {
            file: PathBuf::from("src/lib.rs"),
            executable_lines: vec![1],
            missing_lines: vec![],
        },
        RustLineCoverage {
            file: PathBuf::from("src/lib.rs"),
            executable_lines: vec![2],
            missing_lines: vec![2],
        },
    ];

    let analysis = analysis_from_line_coverage(&parsed, &coverage);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "llvm_cov_missing");
    assert_eq!(
        analysis.unreferenced[0].file,
        PathBuf::from("/repo/src/lib.rs")
    );
}

#[test]
fn runtime_rust_analysis_fails_closed_when_cov_command_cannot_export_json() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    unsafe { std::env::remove_var("CARGO_LLVM_COV_TARGET_DIR") };
    let tmp = tempfile::TempDir::new().unwrap();
    let parsed = vec![parsed_rs(tmp.path().join("src/lib.rs"))];

    let analysis = runtime_rust_analysis(tmp.path(), &parsed);

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.unreferenced[0].name, "llvm_cov_failed");
    assert_eq!(analysis.unreferenced[0].file, tmp.path().join("src/lib.rs"));
}
