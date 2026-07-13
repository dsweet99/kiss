use std::process::{Command, Stdio};
use std::time::Duration;

use super::{BatchProcessTreeGuard, ProcessGroupIdentity, record_child_process_group};

#[test]
fn process_tree_registry_records_and_counts_residuals() {
    let registry = super::ProcessTreeRegistry::default();
    assert!(registry.identities().is_empty());
    assert_eq!(registry.residual_count(), 0);

    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 0.2");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(guard.registry().as_ref(), &child);
    let identities = guard.registry().identities();
    assert_eq!(identities.len(), 1);
    assert!(super::identity_still_valid(&identities[0]));
    assert_eq!(guard.registry().residual_count(), 1);
    child.wait().expect("wait child");
    assert_eq!(guard.registry().residual_count(), 0);
}

#[test]
fn identity_still_valid_rejects_zero_pid_or_pgid() {
    assert!(!super::identity_still_valid(&ProcessGroupIdentity { pid: 0, pgid: 1 }));
    assert!(!super::identity_still_valid(&ProcessGroupIdentity { pid: 1, pgid: 0 }));
}

#[test]
fn process_group_identity_supports_equality_and_debug() {
    let identity = ProcessGroupIdentity { pid: 42, pgid: 42 };
    assert_eq!(identity, identity);
    assert!(format!("{identity:?}").contains("42"));
}

#[test]
fn batch_process_tree_guard_records_child_process_group() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 0.05");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(registry.as_ref(), &child);
    assert!(!registry.identities().is_empty());
    child.wait().expect("wait child");
    assert_eq!(registry.residual_count(), 0);
}

#[test]
fn batch_process_tree_guard_reinstalls_after_drop() {
    {
        let _guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    }
    let guard = BatchProcessTreeGuard::install().expect("reinstall process tree guard");
    assert!(!guard.interrupted());
}

#[test]
fn batch_process_tree_guard_marks_interrupted_after_terminate() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    assert!(!guard.interrupted());
    let _ = guard.terminate_descendants(Duration::from_millis(1));
    assert!(guard.interrupted());
}

#[cfg(unix)]
#[test]
fn batch_process_tree_guard_terminates_signal_ignoring_descendant() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("trap '' INT; sleep 60");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(registry.as_ref(), &child);
    let residuals = guard.terminate_descendants(Duration::from_millis(500));
    match child.wait() {
        Ok(_) => {}
        Err(err) if err.raw_os_error() == Some(libc::ECHILD) => {}
        Err(err) => panic!("wait terminated child: {err}"),
    }
    assert_eq!(
        residuals, 0,
        "SIG_IGN intermediary must still be reaped via escalation"
    );
}

#[cfg(unix)]
#[test]
fn identity_still_valid_rejects_reused_pid_with_mismatched_pgid() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 0.05");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(registry.as_ref(), &child);
    let identity = registry.identities().pop().expect("recorded identity");
    assert!(super::identity_still_valid(&identity));
    child.wait().expect("wait child");
    assert!(!super::identity_still_valid(&identity));
}

#[test]
fn batch_process_tree_guard_terminates_sleeping_descendant() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 30");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(registry.as_ref(), &child);
    let residuals = guard.terminate_descendants(Duration::from_millis(500));
    match child.wait() {
        Ok(_) => {}
        Err(err) if err.raw_os_error() == Some(libc::ECHILD) => {}
        Err(err) => panic!("wait terminated child: {err}"),
    }
    assert_eq!(residuals, 0);
}
