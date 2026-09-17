use super::test_support::{EnvGuard, temp_dir};
use super::*;
use std::fs;
use std::path::Path;
use std::process::Command;
#[cfg(debug_assertions)]
use std::thread;
#[cfg(debug_assertions)]
use std::time::Duration;

fn run_env_body_in_isolated_process(test_name: &str) -> bool {
    const ENV: &str = "KISS_ISOLATED_BARRIER_ENV_TEST";
    if std::env::var(ENV).as_deref() == Ok(test_name) {
        return false;
    }
    let full_name = format!("kiss_publication_barrier::tests::{test_name}");
    let status = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", &full_name])
        .env(ENV, test_name)
        .status()
        .expect("spawn isolated barrier test");
    assert!(status.success(), "isolated barrier test failed: {status}");
    true
}

#[cfg(debug_assertions)]
fn short_policy() -> WaitPolicy {
    WaitPolicy {
        poll_interval: Duration::from_millis(2),
        timeout: Duration::from_millis(50),
    }
}

#[cfg(debug_assertions)]
#[test]
fn non_target_debug_calls_are_no_ops() {
    if run_env_body_in_isolated_process("non_target_debug_calls_are_no_ops") {
        return;
    }
    let dir = temp_dir();
    let _env = EnvGuard::set(Some(&dir), Some("other:after_sync_before_rename"));
    wait_if_targeted(
        "artifact",
        "after_sync_before_rename",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
        short_policy(),
    )
    .unwrap();
    assert!(fs::read_dir(&dir).unwrap().next().is_none());
}

#[test]
fn public_after_sync_is_noop_without_barrier_dir() {
    if run_env_body_in_isolated_process("public_after_sync_is_noop_without_barrier_dir") {
        return;
    }
    let _env = EnvGuard::set(None, Some("artifact:after_sync_before_rename"));
    after_sync_before_rename(
        "artifact",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
    )
    .unwrap();
}

#[cfg(debug_assertions)]
#[test]
fn configured_barrier_dir_rejects_file_path() {
    if run_env_body_in_isolated_process("configured_barrier_dir_rejects_file_path") {
        return;
    }
    let dir = temp_dir();
    let file = dir.join("not-a-dir");
    fs::write(&file, "").unwrap();
    let _env = EnvGuard::set(Some(&file), Some("artifact:after_rename"));
    let err = wait_if_targeted(
        "artifact",
        "after_rename",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
        short_policy(),
    )
    .expect_err("file barrier dir must fail");
    assert!(err.to_string().contains("not a directory"));
}

#[cfg(debug_assertions)]
#[test]
fn targeted_call_publishes_ready_and_waits_for_matching_release() {
    if run_env_body_in_isolated_process(
        "targeted_call_publishes_ready_and_waits_for_matching_release",
    ) {
        return;
    }
    let dir = temp_dir();
    let _env = EnvGuard::set(Some(&dir), Some("artifact:after_sync_before_rename"));
    let dir_for_thread = dir.clone();
    let handle = thread::spawn(move || {
        loop {
            let ready = fs::read_dir(&dir_for_thread)
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().ends_with(".ready.json"));
            if let Some(ready) = ready {
                let text = fs::read_to_string(ready.path()).unwrap();
                let operation_id = json_string_field(&text, "operation_id").unwrap();
                let release = dir_for_thread.join(format!("{operation_id}.release.json"));
                fs::write(
                    release,
                    format!(
                        "{{\"schema_version\":1,\"operation_id\":\"{}\",\"artifact\":\"artifact\",\"phase\":\"after_sync_before_rename\"}}\n",
                        json_escape(&operation_id)
                    ),
                )
                .unwrap();
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
    });
    wait_if_targeted(
        "artifact",
        "after_sync_before_rename",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
        WaitPolicy {
            poll_interval: Duration::from_millis(2),
            timeout: Duration::from_secs(2),
        },
    )
    .unwrap();
    handle.join().unwrap();
    let ready_count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".ready.json"))
        .count();
    assert_eq!(ready_count, 1);
}

#[cfg(debug_assertions)]
#[test]
fn malformed_matching_release_fails_clearly() {
    let record = ReleaseRecord {
        schema_version: 1,
        operation_id: "wrong".to_string(),
        artifact: "artifact".to_string(),
        phase: "after_rename".to_string(),
    };
    let err = validate_release_record(&record, "op", "artifact", "after_rename")
        .expect_err("mismatch must fail");
    assert!(err.to_string().contains("malformed publication barrier"));
}

#[cfg(debug_assertions)]
#[test]
fn missing_release_times_out_clearly() {
    if run_env_body_in_isolated_process("missing_release_times_out_clearly") {
        return;
    }
    let dir = temp_dir();
    let _env = EnvGuard::set(Some(&dir), Some("artifact:after_rename"));
    let err = wait_if_targeted(
        "artifact",
        "after_rename",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
        short_policy(),
    )
    .expect_err("missing release must time out");
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(err.to_string().contains("timed out waiting"));
}

#[cfg(debug_assertions)]
#[test]
fn mismatched_release_name_times_out() {
    if run_env_body_in_isolated_process("mismatched_release_name_times_out") {
        return;
    }
    let dir = temp_dir();
    let _env = EnvGuard::set(Some(&dir), Some("artifact:after_rename"));
    fs::write(
        dir.join("unrelated.release.json"),
        "{\"schema_version\":1,\"operation_id\":\"unrelated\",\"artifact\":\"artifact\",\"phase\":\"after_rename\"}\n",
    )
    .unwrap();
    let err = wait_if_targeted(
        "artifact",
        "after_rename",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
        short_policy(),
    )
    .expect_err("mismatched release file must time out");
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}

#[cfg(debug_assertions)]
#[test]
fn generated_paths_stay_below_barrier_dir() {
    let dir = temp_dir().canonicalize().unwrap();
    let op = operation_id("../artifact", "../phase", Path::new("../../bad.tmp"));
    let ready = dir.join(format!("{op}.ready.json"));
    let release = dir.join(format!("{op}.release.json"));
    ensure_child_path(&dir, &ready).unwrap();
    ensure_child_path(&dir, &release).unwrap();
    assert!(!op.contains('/'));
    assert!(!op.contains('\\'));
}

#[cfg(all(debug_assertions, unix))]
#[test]
fn symlink_release_is_rejected() {
    let dir = temp_dir();
    let release = dir.join("op.release.json");
    let outside = dir.join("outside.json");
    fs::write(&outside, "{}").unwrap();
    std::os::unix::fs::symlink(&outside, &release).unwrap();
    let err = read_release_record(&release).expect_err("symlink release must fail");
    assert!(err.to_string().contains("symlink"));
}

#[cfg(debug_assertions)]
#[test]
fn directory_release_is_rejected() {
    let dir = temp_dir();
    let release = dir.join("op.release.json");
    fs::create_dir(&release).unwrap();
    let err = read_release_record(&release).expect_err("directory release must fail");
    assert!(err.to_string().contains("not a file"));
}

#[cfg(debug_assertions)]
#[test]
fn json_helpers_escape_and_reject_invalid_fields() {
    assert_eq!(json_escape("a\"b\\c\n\t\u{7}"), "a\\\"b\\\\c\\n\\t\\u0007");
    assert_eq!(
        json_string_field(r#"{"field":"a\"b\/c\n"}"#, "field").unwrap(),
        "a\"b/c\n"
    );
    assert!(json_number_field(r#"{"field":"nope"}"#, "field").is_err());
    assert!(json_number_field(r#"{}"#, "field").is_err());
    assert!(json_string_field(r#"{"field":1}"#, "field").is_err());
    assert!(json_string_field(r#"{"field":"unterminated}"#, "field").is_err());
}

#[cfg(debug_assertions)]
#[test]
fn repeated_operations_do_not_collide() {
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..100 {
        ids.insert(operation_id(
            "artifact",
            "after_rename",
            Path::new("/tmp/a.tmp"),
        ));
    }
    assert_eq!(ids.len(), 100);
}

#[cfg(debug_assertions)]
#[test]
fn release_build_public_calls_are_no_ops_even_when_configured() {
    if run_env_body_in_isolated_process(
        "release_build_public_calls_are_no_ops_even_when_configured",
    ) {
        return;
    }
    let dir = temp_dir();
    let _env = EnvGuard::set(Some(&dir), Some("artifact:after_rename"));
    let result = wait_if_targeted(
        "artifact",
        "after_rename",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
        short_policy(),
    );
    assert!(result.is_err());
}

#[cfg(not(debug_assertions))]
#[test]
fn release_build_public_calls_are_no_ops_even_when_configured() {
    let dir = temp_dir();
    let _env = EnvGuard::set(Some(&dir), Some("artifact:after_rename"));
    after_rename(
        "artifact",
        Path::new("/tmp/a.tmp"),
        Path::new("/tmp/a.json"),
    )
    .unwrap();
}
