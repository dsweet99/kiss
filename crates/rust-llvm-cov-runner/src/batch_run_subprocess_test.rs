use std::collections::{BTreeMap, HashSet};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::batch_process_tree::{BatchProcessTreeGuard, record_child_process_group};
use crate::batch_shim::write_shim_start_metadata;
use crate::batch_shim_delegated::COVERAGE_BUILD_ENV_KEYS_FOR_TEST;

use super::{
    apply_batch_subprocess_env, ingest_live_shim_identities, wait_child_with_interruption,
};

#[test]
fn apply_batch_subprocess_env_scrubs_inherited_llvm_profile_file() {
    let mut command = Command::new("/bin/true");
    command.env("LLVM_PROFILE_FILE", "/tmp/outer-should-not-leak.profraw");
    let plan_env = BTreeMap::from([("CARGO_TARGET_DIR".to_string(), "/tmp/nested".to_string())]);
    apply_batch_subprocess_env(&mut command, &plan_env);
    assert_env_removed(&command, "LLVM_PROFILE_FILE");
    assert_eq!(
        command
            .get_envs()
            .find(|(key, _)| key.to_string_lossy() == "CARGO_TARGET_DIR")
            .and_then(|(_, value)| value.map(|v| v.to_string_lossy().into_owned())),
        Some("/tmp/nested".to_string())
    );
}

#[test]
fn metamorphic_apply_batch_subprocess_env_scrubs_all_coverage_keys() {
    let mut command = Command::new("/bin/true");
    for key in COVERAGE_BUILD_ENV_KEYS_FOR_TEST {
        command.env(key, format!("stale-{key}"));
    }
    let plan_env = BTreeMap::from([
        (
            "CARGO_TARGET_DIR".to_string(),
            "/tmp/plan-target".to_string(),
        ),
        ("KEEP_ME".to_string(), "1".to_string()),
    ]);
    apply_batch_subprocess_env(&mut command, &plan_env);
    for key in COVERAGE_BUILD_ENV_KEYS_FOR_TEST {
        if *key == "CARGO_TARGET_DIR" {
            continue;
        }
        assert_env_removed(&command, key);
    }
    assert_eq!(
        env_value(&command, "CARGO_TARGET_DIR").as_deref(),
        Some("/tmp/plan-target")
    );
    assert_eq!(env_value(&command, "KEEP_ME").as_deref(), Some("1"));
}

#[test]
fn fuzz_apply_batch_subprocess_env_clears_random_coverage_key_subsets() {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    println!("fuzz_apply_batch_subprocess_env seed={seed}");
    let mut state = seed;
    for round in 0..32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut command = Command::new("/bin/true");
        let mut expected_removed = Vec::new();
        for (index, key) in COVERAGE_BUILD_ENV_KEYS_FOR_TEST.iter().enumerate() {
            if (state >> index) & 1 == 1 {
                command.env(key, format!("round-{round}-{key}"));
                expected_removed.push(*key);
            }
        }
        apply_batch_subprocess_env(&mut command, &BTreeMap::new());
        for key in expected_removed {
            assert_env_removed(&command, key);
        }
    }
}

fn assert_env_removed(command: &Command, key: &str) {
    let entry = command
        .get_envs()
        .find(|(candidate, _)| candidate.to_string_lossy() == key);
    assert!(
        matches!(entry, Some((_, None))),
        "{key} must be scrubbed from nested batch command env"
    );
}

fn env_value(command: &Command, key: &str) -> Option<String> {
    command
        .get_envs()
        .find(|(candidate, _)| candidate.to_string_lossy() == key)
        .and_then(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
}

#[test]
fn ingest_live_shim_identities_records_each_identity_once() {
    let guard = BatchProcessTreeGuard::install().expect("install guard");
    let mut children = Vec::new();
    let mut identities = Vec::new();
    for _ in 0..2 {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 0.5");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let child = guard
            .spawn_batch_command(&mut command)
            .expect("spawn child");
        record_child_process_group(guard.registry().as_ref(), &child);
        identities.push(
            guard
                .registry()
                .identities()
                .last()
                .expect("recorded child identity")
                .clone(),
        );
        children.push(child);
    }
    let registry = crate::batch_process_tree::ProcessTreeRegistry::default();
    let tmp = tempfile::tempdir().unwrap();
    write_shim_start_metadata(tmp.path(), "alpha", &identities[0]).unwrap();
    write_shim_start_metadata(tmp.path(), "beta", &identities[1]).unwrap();
    let mut seen = HashSet::new();
    ingest_live_shim_identities(&registry, tmp.path(), &mut seen);
    ingest_live_shim_identities(&registry, tmp.path(), &mut seen);
    let recorded = registry.identities();
    assert_eq!(recorded.len(), 2);
    assert!(recorded.contains(&identities[0]));
    assert!(recorded.contains(&identities[1]));
    for mut child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[test]
fn wait_child_with_interruption_returns_on_normal_exit() {
    let guard = BatchProcessTreeGuard::install().expect("install guard");
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 0.05");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(guard.registry().as_ref(), &child);
    let mut seen = HashSet::new();
    let status = wait_child_with_interruption(
        &mut child,
        &guard,
        std::path::Path::new("/nonexistent"),
        &mut seen,
    )
    .expect("wait child");
    assert!(status.success());
    assert!(!guard.interrupted());
}

#[test]
fn wait_child_with_interruption_fails_when_interrupted() {
    let guard = BatchProcessTreeGuard::install().expect("install guard");
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 2");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(guard.registry().as_ref(), &child);
    guard.set_interrupted_for_test(true);
    let mut seen = HashSet::new();
    let err = wait_child_with_interruption(
        &mut child,
        &guard,
        std::path::Path::new("/nonexistent"),
        &mut seen,
    )
    .expect_err("expected interrupted wait");
    assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
    assert!(err.to_string().contains("batch interrupted"));
}
