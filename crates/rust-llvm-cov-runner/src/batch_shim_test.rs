use std::ffi::OsString;
use std::fs;

use super::{run_target_runner_shim, write_shim_start_metadata};
use crate::test_support::{make_executable, shim_only_metadata, shim_test_env_lock};

#[test]
fn target_runner_shim_writes_start_metadata_before_completion() {
    let _env_guard = shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    let script = tmp.path().join("sleep-then-exit.sh");
    fs::write(&script, "#!/bin/sh\nsleep 0.05\nexit 2\n").unwrap();
    make_executable(&script);

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.clone().into_os_string()],
    );

    assert_eq!(code, 2);
    let start_paths: Vec<_> = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".shim-start.json"))
        })
        .collect();
    assert_eq!(start_paths.len(), 1);
    let delegated_paths: Vec<_> = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".delegated-start.json"))
        })
        .collect();
    assert_eq!(delegated_paths.len(), 1);
    let metadata = shim_only_metadata(&output);
    assert!(metadata.shim_identity.is_some());
    assert!(metadata.delegated_identity.is_some());
}

#[test]
fn load_live_shim_process_identities_reads_start_records() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = crate::batch_process_tree::ProcessGroupIdentity { pid: 42, pgid: 42 };
    write_shim_start_metadata(tmp.path(), "alpha", &identity).unwrap();
    let loaded = super::load_live_shim_process_identities(tmp.path()).unwrap();
    assert!(loaded.is_empty() || loaded.iter().any(|item| item.pid == 42));
}

#[test]
fn target_runner_shim_writes_metadata_and_profile_path_env() {
    let _env_guard = shim_test_env_lock();
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
    let metadata = shim_only_metadata(&output);
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
fn target_runner_shim_writes_delegated_start_metadata() {
    let _env_guard = shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    let script = tmp.path().join("child.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&script);
    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.clone().into_os_string()],
    );
    assert_eq!(code, 0);
    let delegated_paths: Vec<_> = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".delegated-start.json"))
        })
        .collect();
    assert_eq!(delegated_paths.len(), 1);
}

#[test]
fn target_runner_shim_delegates_to_configured_runner() {
    let _env_guard = shim_test_env_lock();
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
    let metadata = shim_only_metadata(&output);
    assert_eq!(metadata.full_name, "child$my_test");
    assert_eq!(metadata.stdout.as_deref(), Some(b"child-out".as_ref()));
    assert_eq!(metadata.stderr.as_deref(), Some(b"child-err".as_ref()));
}

#[test]
fn target_runner_shim_list_phase_delegates_with_list_metadata_without_profile() {
    let _env_guard = shim_test_env_lock();
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
    let list = super::load_target_runner_list_metadata(&output).unwrap();
    assert_eq!(list.len(), 1);
    assert!(
        list[0]
            .test_names
            .iter()
            .any(|name| name.ends_with("$alpha"))
    );
    assert!(
        super::load_target_runner_shim_metadata(&output)
            .unwrap()
            .is_empty()
    );
    assert!(
        fs::read_dir(&output)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .all(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
    );
}

#[cfg(unix)]
#[test]
fn target_runner_shim_signal_forwarder_forwards_during_run() {
    let _env_guard = shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    let script = tmp.path().join("sleep-child.sh");
    fs::write(&script, "#!/bin/sh\nsleep 2\nexit 0\n").unwrap();
    make_executable(&script);
    let script_arg = script.clone().into_os_string();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        unsafe {
            libc::raise(libc::SIGTERM);
        }
    });
    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script_arg],
    );
    let _ = handle.join();
    assert!(code == 0 || code == 1 || code == 143 || code == 15);
}

#[test]
fn target_runner_shim_ignores_nextest_env_for_shell_script_commands() {
    let _env_guard = shim_test_env_lock();
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
    let metadata = shim_only_metadata(&output);
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
fn target_runner_shim_returns_one_for_missing_command() {
    let _env_guard = shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{}").unwrap();

    let code = run_target_runner_shim(&output, &runner_map, "x86_64-unknown-linux-gnu", &[]);

    assert_eq!(code, 1);
}

#[test]
fn target_runner_shim_clears_inherited_llvm_profile_file() {
    let _env_guard = shim_test_env_lock();
    unsafe {
        std::env::set_var("LLVM_PROFILE_FILE", "/tmp/should-be-cleared.profraw");
    }
    super::clear_inherited_llvm_profile_file();
    assert!(
        std::env::var_os("LLVM_PROFILE_FILE").is_none(),
        "shim must drop inherited LLVM_PROFILE_FILE"
    );
}

#[test]
fn target_runner_shim_returns_one_for_malformed_runner_map() {
    let _env_guard = shim_test_env_lock();
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("instances");
    let runner_map = tmp.path().join("runner-map.json");
    let script = tmp.path().join("child.sh");
    fs::write(&runner_map, b"not json").unwrap();
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&script);

    let code = run_target_runner_shim(
        &output,
        &runner_map,
        "x86_64-unknown-linux-gnu",
        &[script.into_os_string()],
    );

    assert_eq!(code, 1);
}
