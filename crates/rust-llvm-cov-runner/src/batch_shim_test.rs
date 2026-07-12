use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::{
    BatchShimMetadata, load_target_runner_shim_metadata, run_target_runner_shim,
    write_shim_metadata,
};
use crate::batch_output_channel::{
    OutputChannelServer, apply_output_channel_env, create_output_channel_config,
};
use crate::test_support::make_executable;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn target_runner_shim_writes_metadata_and_profile_path_env() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    let script = tmp.path().join("check-profile.sh");
    fs::write(
        &script,
        "#!/bin/sh\n[ -n \"$LLVM_PROFILE_FILE\" ] || exit 9\nprintf profile > \"$LLVM_PROFILE_FILE\"\nexit 7\n",
    )
    .unwrap();
    make_executable(&script);

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.clone().into_os_string()],
    );

    assert_eq!(code, 7);
    let metadata = only_metadata(&output);
    assert_eq!(metadata.exit_code, Some(7));
    assert!(metadata.shim_identity.is_some());
    assert!(metadata.delegated_identity.is_some());
    assert_eq!(metadata.argv, [script.to_string_lossy().to_string()]);
    assert_eq!(
        fs::read_to_string(metadata.profile_path).unwrap(),
        "profile"
    );
}

#[test]
fn target_runner_shim_delegates_to_configured_runner() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    let wrapper = tmp.path().join("wrapper.sh");
    let script = tmp.path().join("child.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf child-out\nprintf child-err 1>&2\nexit 4\n",
    )
    .unwrap();
    fs::write(&wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    make_executable(&script);
    make_executable(&wrapper);
    fs::write(
        &runner_map,
        format!(
            r#"{{"x86_64-unknown-linux-gnu":["{}"]}}"#,
            wrapper.to_string_lossy()
        ),
    )
    .unwrap();

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[
            script.clone().into_os_string(),
            OsString::from("--exact"),
            OsString::from("my_test"),
        ],
    );

    assert_eq!(code, 4);
    let metadata = only_metadata(&output);
    assert_eq!(metadata.full_name, "child$my_test");
    assert_eq!(metadata.stdout.as_deref(), Some(b"child-out".as_ref()));
    assert_eq!(metadata.stderr.as_deref(), Some(b"child-err".as_ref()));
}

#[test]
fn target_runner_shim_list_phase_delegates_without_run_metadata_or_profile() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    let marker = tmp.path().join("marker");
    let script = tmp.path().join("list-child.sh");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ -z \"${{LLVM_PROFILE_FILE:-}}\" ] || exit 9\nprintf listed > \"{}\"\nprintf '{{\"type\":\"test\",\"event\":\"discovered\",\"name\":\"alpha\"}}\\n'\nexit 0\n",
            marker.display()
        ),
    )
    .unwrap();
    make_executable(&script);

    // SAFETY: the lock serializes test-only mutation of process-wide nextest env.
    unsafe {
        std::env::set_var("NEXTEST_TEST_PHASE", "list");
    }
    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.clone().into_os_string()],
    );
    // SAFETY: the lock serializes test-only mutation of process-wide nextest env.
    unsafe {
        std::env::remove_var("NEXTEST_TEST_PHASE");
    }

    assert_eq!(code, 0);
    assert_eq!(fs::read_to_string(marker).unwrap(), "listed");
    assert!(!output.exists());
}

#[test]
fn target_runner_shim_ignores_nextest_env_for_shell_script_commands() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{}").unwrap();
    let script = tmp.path().join("check-profile.sh");
    fs::write(&script, "#!/bin/sh\nexit 8\n").unwrap();
    make_executable(&script);

    // SAFETY: test-only env mutation restored by process exit.
    unsafe {
        std::env::set_var("NEXTEST_TEST_PHASE", "run");
        std::env::set_var("NEXTEST_BINARY_ID", "kiss-ai::bin/kiss");
        std::env::set_var(
            "NEXTEST_TEST_NAME",
            "bin_cli::run::run_coverage::hidden_rust_llvm_cov_target_runner_dispatches_before_config_loading",
        );
    }

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.clone().into_os_string()],
    );

    assert_eq!(code, 8);
    let metadata = only_metadata(&output);
    assert_ne!(
        metadata.id,
        "kiss-ai::bin/kiss$bin_cli::run::run_coverage::hidden_rust_llvm_cov_target_runner_dispatches_before_config_loading"
    );
    assert!(metadata.profile_path.starts_with(&output));
    // SAFETY: the lock serializes test-only mutation of process-wide nextest env.
    unsafe {
        std::env::remove_var("NEXTEST_TEST_PHASE");
        std::env::remove_var("NEXTEST_BINARY_ID");
        std::env::remove_var("NEXTEST_TEST_NAME");
    }
}

#[test]
fn target_runner_shim_routes_child_output_through_side_channel_only() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    let script = tmp.path().join("child.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf child-out\nprintf child-err 1>&2\nexit 3\n",
    )
    .unwrap();
    make_executable(&script);

    let channel_config = create_output_channel_config(tmp.path(), false).unwrap();
    let server = OutputChannelServer::start(channel_config.clone()).unwrap();
    let mut env = std::collections::BTreeMap::new();
    apply_output_channel_env(&mut env, &channel_config);
    for (key, value) in env {
        // SAFETY: test-only env mutation serialized by ENV_LOCK.
        unsafe {
            std::env::set_var(key, value);
        }
    }

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.clone().into_os_string()],
    );

    // SAFETY: test-only env mutation serialized by ENV_LOCK.
    unsafe {
        std::env::remove_var(crate::batch_output_channel::OUTPUT_CHANNEL_SOCKET_ENV);
        std::env::remove_var(crate::batch_output_channel::OUTPUT_CHANNEL_TOKEN_ENV);
    }

    assert_eq!(code, 3);
    let metadata = only_metadata(&output);
    assert_eq!(metadata.stdout.as_deref(), Some(b"child-out".as_ref()));
    assert_eq!(metadata.stderr.as_deref(), Some(b"child-err".as_ref()));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let frames = server.stop();
    assert_eq!(frames.len(), 2);
    assert!(frames.iter().any(|frame| frame.bytes == b"child-out"
        && frame.stream == crate::batch_output_channel::OutputStreamKind::Stdout));
    assert!(frames.iter().any(|frame| frame.bytes == b"child-err"
        && frame.stream == crate::batch_output_channel::OutputStreamKind::Stderr));
}

#[test]
fn target_runner_shim_reports_missing_command() {
    let tmp = tempfile::tempdir().unwrap();
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{}").unwrap();

    let code = run_target_runner_shim(tmp.path(), &runner_map, "x86_64-unknown-linux-gnu", &[]);

    assert_eq!(code, 1);
}

#[test]
fn target_runner_shim_uses_exact_test_name_from_command() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{}").unwrap();
    let script = tmp.path().join("child.sh");
    fs::write(&script, "#!/bin/sh\nexit 4\n").unwrap();
    make_executable(&script);

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[
            script.clone().into_os_string(),
            OsString::from("--exact"),
            OsString::from("my_test"),
        ],
    );

    assert_eq!(code, 4);
    let metadata = only_metadata(&output);
    assert_eq!(metadata.full_name, "child$my_test");
}

#[test]
fn shim_metadata_round_trips_through_json_file() {
    let tmp = tempfile::tempdir().unwrap();
    let metadata = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v2".to_string(),
        id: "alpha".to_string(),
        full_name: "pkg::bin$alpha".to_string(),
        profile_path: tmp.path().join("alpha.profraw"),
        cwd: tmp.path().to_path_buf(),
        argv: vec![
            "bin".to_string(),
            "--exact".to_string(),
            "alpha".to_string(),
        ],
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: Some(b"out".to_vec()),
        stderr: Some(b"err".to_vec()),
    };
    write_shim_metadata(tmp.path(), "alpha", &metadata).unwrap();
    let loaded = load_target_runner_shim_metadata(tmp.path()).unwrap();
    assert_eq!(loaded, vec![metadata]);
}

#[test]
fn load_target_runner_shim_metadata_reads_sorted_json_records() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path()).unwrap();
    let first = metadata("b");
    let second = metadata("a");
    fs::write(
        tmp.path().join("b.json"),
        serde_json::to_vec(&first).unwrap(),
    )
    .unwrap();
    fs::write(
        tmp.path().join("a.json"),
        serde_json::to_vec(&second).unwrap(),
    )
    .unwrap();
    fs::write(tmp.path().join("ignore.profraw"), b"profile").unwrap();

    let loaded = load_target_runner_shim_metadata(tmp.path()).unwrap();

    assert_eq!(
        loaded.into_iter().map(|item| item.id).collect::<Vec<_>>(),
        ["a", "b"]
    );
}

fn only_metadata(output: &Path) -> BatchShimMetadata {
    let paths = fs::read_dir(output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1);
    serde_json::from_slice(&fs::read(&paths[0]).unwrap()).unwrap()
}

fn metadata(id: &str) -> BatchShimMetadata {
    BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v2".to_string(),
        id: id.to_string(),
        full_name: id.to_string(),
        profile_path: PathBuf::from(format!("{id}.profraw")),
        cwd: PathBuf::from("/repo"),
        argv: vec!["test-bin".to_string()],
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: None,
        stderr: None,
    }
}
