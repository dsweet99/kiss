use std::process::{Command, Stdio};
use std::time::Duration;

use super::{BatchProcessTreeGuard, ProcessGroupIdentity, record_child_process_group};

#[test]
fn configure_batch_child_process_group_records_current_identity() {
    let registry = super::ProcessTreeRegistry::default();
    super::configure_batch_child_process_group(&registry).expect("configure group");
    let identities = registry.identities();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].pid, std::process::id());
    assert!(super::identity_still_valid(&identities[0]));
}

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
    assert!(!super::identity_still_valid(&ProcessGroupIdentity {
        pid: 0,
        pgid: 1
    }));
    assert!(!super::identity_still_valid(&ProcessGroupIdentity {
        pid: 1,
        pgid: 0
    }));
}

#[test]
fn process_tree_registry_records_explicit_identities() {
    let registry = super::ProcessTreeRegistry::default();
    let identity = ProcessGroupIdentity {
        pid: std::process::id(),
        pgid: unsafe { libc::getpgid(0) as u32 },
    };

    registry.record(identity.clone());

    assert_eq!(registry.identities(), vec![identity]);
}

#[test]
fn process_group_signal_helpers_ignore_invalid_inputs() {
    super::signal_validated_process_group(&ProcessGroupIdentity { pid: 0, pgid: 1 }, libc::SIGTERM);
    super::signal_process_group(0, libc::SIGTERM);
    assert!(!super::process_group_alive(0));
}

#[test]
fn process_group_identity_supports_equality_and_debug() {
    let identity = ProcessGroupIdentity { pid: 42, pgid: 42 };
    assert_eq!(identity, identity);
    assert!(format!("{identity:?}").contains("42"));
}

#[cfg(unix)]
#[test]
fn record_current_process_group_records_this_process() {
    let registry = super::ProcessTreeRegistry::default();

    super::record_current_process_group(&registry);

    let identities = registry.identities();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].pid, std::process::id());
    assert!(identities[0].pgid > 0);
}

#[test]
fn reap_zombies_is_idempotent_when_no_children_are_waiting() {
    super::batch_process_tree_reap::reap_zombies();
    super::batch_process_tree_reap::reap_zombies();
    assert_eq!(super::batch_process_tree_reap::reap_zombies_count(), 0);
}

#[test]
fn reap_zombies_reaps_exited_child_processes() {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn short child");
    std::thread::sleep(Duration::from_millis(50));

    let reaped = super::batch_process_tree_reap::reap_zombies_count();

    match child.try_wait() {
        Ok(Some(_)) => assert_eq!(reaped, 0),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child should have exited before reap");
        }
        Err(err) if err.raw_os_error() == Some(libc::ECHILD) => assert!(reaped >= 1),
        Err(err) => panic!("try_wait after reap: {err}"),
    }
}

#[cfg(unix)]
#[test]
fn reap_zombies_count_counts_forked_exited_child() {
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        unsafe { libc::_exit(0) };
    }
    std::thread::sleep(Duration::from_millis(50));

    let reaped = super::batch_process_tree_reap::reap_zombies_count();

    assert!(reaped >= 1);
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
fn batch_scope_interrupt_guard_installs_shared_state() {
    let _scope = super::BatchScopeInterruptGuard::install().expect("install scope guard");
    let guard = BatchProcessTreeGuard::install().expect("install scoped process tree guard");

    assert!(!super::batch_scope_interrupted());
    guard.set_interrupted_for_test(true);
    assert!(super::batch_scope_interrupted());
}

#[test]
fn batch_process_tree_guard_marks_interrupted_after_terminate() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    assert!(!guard.interrupted());
    let _ = guard.terminate_descendants(Duration::from_millis(1));
    assert!(guard.interrupted());
}

#[test]
fn reap_lingering_descendants_does_not_mark_interrupted() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    assert!(!guard.interrupted());
    let _ = guard.reap_lingering_descendants(Duration::from_millis(1));
    assert!(!guard.interrupted());
}

#[cfg(unix)]
#[test]
fn batch_process_tree_guard_terminates_signal_ignoring_descendant() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("trap '' INT; sleep 60");
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

#[test]
fn batch_process_tree_guard_drop_terminates_recorded_child() {
    let mut child = {
        let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
        let registry = guard.registry();
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 30");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let child = guard
            .spawn_batch_command(&mut command)
            .expect("spawn child");
        record_child_process_group(registry.as_ref(), &child);
        child
    };

    match child.wait() {
        Ok(_) => {}
        Err(err) if err.raw_os_error() == Some(libc::ECHILD) => {}
        Err(err) => panic!("wait drop-terminated child: {err}"),
    }
}
