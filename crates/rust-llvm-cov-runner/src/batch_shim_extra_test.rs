use std::ffi::OsString;
use std::fs;

use super::{
    BatchShimDelegatedStartMetadata, BatchShimMetadata, BatchShimStartMetadata,
    load_target_runner_shim_metadata, run_target_runner_shim, write_shim_metadata,
};
use crate::batch_output_channel::{
    OutputChannelServer, apply_output_channel_env, create_output_channel_config,
};
use crate::batch_shim::batch_shim_child::build_delegated_command;
use crate::test_support::{make_executable, shim_metadata, shim_only_metadata, shim_test_env_lock};

#[test]
fn target_runner_shim_routes_child_output_through_side_channel_only() {
    let _env_guard = shim_test_env_lock();
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
    let metadata = shim_only_metadata(&output);
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
fn shim_start_and_delegated_start_metadata_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = crate::batch_process_tree::ProcessGroupIdentity { pid: 99, pgid: 99 };
    let start = BatchShimStartMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-start-v1".to_string(),
        id: "alpha".to_string(),
        shim_identity: identity.clone(),
    };
    let delegated = BatchShimDelegatedStartMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-delegated-start-v1".to_string(),
        id: "alpha".to_string(),
        delegated_identity: identity,
    };
    fs::write(
        tmp.path().join("alpha.shim-start.json"),
        serde_json::to_vec(&start).unwrap(),
    )
    .unwrap();
    fs::write(
        tmp.path().join("alpha.delegated-start.json"),
        serde_json::to_vec(&delegated).unwrap(),
    )
    .unwrap();
    let loaded_start: BatchShimStartMetadata =
        serde_json::from_slice(&fs::read(tmp.path().join("alpha.shim-start.json")).unwrap())
            .unwrap();
    let loaded_delegated: BatchShimDelegatedStartMetadata =
        serde_json::from_slice(&fs::read(tmp.path().join("alpha.delegated-start.json")).unwrap())
            .unwrap();
    assert_eq!(loaded_start, start);
    assert_eq!(loaded_delegated, delegated);
}

#[cfg(unix)]
#[test]
fn load_live_shim_process_identities_reads_delegated_start_for_current_process() {
    let tmp = tempfile::tempdir().unwrap();
    let pid = std::process::id();
    let pgid = unsafe { libc::getpgid(0) } as u32;
    let identity = crate::batch_process_tree::ProcessGroupIdentity { pid, pgid };
    let delegated = BatchShimDelegatedStartMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-delegated-start-v1".to_string(),
        id: "alpha".to_string(),
        delegated_identity: identity,
    };
    fs::write(
        tmp.path().join("alpha.delegated-start.json"),
        serde_json::to_vec(&delegated).unwrap(),
    )
    .unwrap();
    let loaded = super::load_live_shim_process_identities(tmp.path()).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].pid, pid);
}

#[cfg(unix)]
#[test]
fn shim_forward_signal_noops_when_delegated_identity_cleared() {
    super::install_shim_signal_forwarder().expect("install shim forwarder");
    super::ShimSignalForwarder::clear_delegated_identity();
    super::trigger_shim_forward_signal_for_test(libc::SIGTERM);
    super::clear_shim_signal_forwarder();
}

#[cfg(unix)]
#[test]
fn shim_forward_signal_forwards_with_delegated_identity_set() {
    super::install_shim_signal_forwarder().expect("install shim forwarder");
    let identity = crate::batch_process_tree::ProcessGroupIdentity {
        pid: 9_999_999,
        pgid: 9_999_999,
    };
    super::ShimSignalForwarder::set_delegated_identity(&identity);
    super::trigger_shim_forward_signal_for_test(libc::SIGTERM);
    super::ShimSignalForwarder::clear_delegated_identity();
    super::clear_shim_signal_forwarder();
}

#[cfg(unix)]
#[test]
fn shim_signal_forwarder_install_and_clear_direct() {
    super::install_shim_signal_forwarder().expect("install shim forwarder");
    super::clear_shim_signal_forwarder();
}

#[test]
fn atomic_metadata_write_fails_when_temp_file_exists() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join(".alpha.json.tmp"), b"{}\n").unwrap();
    let metadata = shim_metadata("alpha");
    let err = write_shim_metadata(tmp.path(), "alpha", &metadata).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
}

#[test]
fn target_runner_shim_reports_missing_delegated_binary_exit_code() {
    let _env_guard = shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{}").unwrap();
    let missing = tmp.path().join("missing-binary");
    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[missing.into_os_string()],
    );
    assert_ne!(code, 0);
    let metadata = shim_only_metadata(&output);
    assert_ne!(metadata.exit_code, Some(0));
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
fn nextest_list_phase_detection_reads_environment() {
    let _env_guard = shim_test_env_lock();
    // SAFETY: serialized by shim_test_env_lock.
    unsafe {
        std::env::remove_var("NEXTEST_TEST_PHASE");
    }
    assert!(!super::batch_shim_child::is_nextest_list_phase());

    // SAFETY: serialized by shim_test_env_lock.
    unsafe {
        std::env::set_var("NEXTEST_TEST_PHASE", "list");
    }
    assert!(super::batch_shim_child::is_nextest_list_phase());

    // SAFETY: serialized by shim_test_env_lock.
    unsafe {
        std::env::remove_var("NEXTEST_TEST_PHASE");
    }
}

#[test]
fn delegated_command_builder_and_runner_map_error_paths_are_covered() {
    let direct = build_delegated_command(&[], &[OsString::from("bin"), OsString::from("arg")]);
    assert_eq!(direct.get_program(), std::ffi::OsStr::new("bin"));
    assert_eq!(
        direct.get_args().collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("arg")]
    );

    let delegated = build_delegated_command(
        &["wrapper".to_string(), "--flag".to_string()],
        &[OsString::from("bin")],
    );
    assert_eq!(delegated.get_program(), std::ffi::OsStr::new("wrapper"));
    assert_eq!(
        delegated.get_args().collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("--flag"), std::ffi::OsStr::new("bin")]
    );

    let tmp = tempfile::tempdir().unwrap();
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{not-json").unwrap();
    let err = super::run_target_runner_shim_inner(
        tmp.path(),
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[OsString::from("bin")],
    )
    .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
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
    let metadata = shim_only_metadata(&output);
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
        output_frame_count: None,
    };
    write_shim_metadata(tmp.path(), "alpha", &metadata).unwrap();
    let loaded = load_target_runner_shim_metadata(tmp.path()).unwrap();
    assert_eq!(loaded, vec![metadata]);
}

#[test]
fn load_target_runner_shim_metadata_reads_sorted_json_records() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path()).unwrap();
    let first = shim_metadata("b");
    let second = shim_metadata("a");
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

#[test]
fn shim_metadata_loaders_return_empty_for_missing_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");

    assert!(
        super::load_target_runner_shim_metadata(&missing)
            .unwrap()
            .is_empty()
    );
    assert!(
        super::load_target_runner_list_metadata(&missing)
            .unwrap()
            .is_empty()
    );
    assert!(
        super::load_live_shim_process_identities(&missing)
            .unwrap()
            .is_empty()
    );
}
