use std::process::{Command, Stdio};
use std::time::Duration;

use super::{BatchProcessTreeGuard, ProcessGroupIdentity, record_child_process_group};

#[test]
fn process_group_identity_supports_equality_and_debug() {
    let identity = ProcessGroupIdentity { pid: 42, pgid: 42 };
    assert_eq!(identity, identity);
    assert!(format!("{identity:?}").contains("42"));
}

#[test]
fn clear_sigint_handler_is_safe_before_install() {
    super::clear_sigint_handler();
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
fn batch_process_tree_guard_sigint_handler_sets_interrupted_without_children() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(50));
        unsafe {
            libc::raise(libc::SIGINT);
        }
    });
    std::thread::sleep(Duration::from_millis(150));
    assert!(guard.interrupted());
}

#[cfg(unix)]
#[test]
fn batch_process_tree_guard_sigint_handler_terminates_recorded_child() {
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("sleep 60");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    let mut child = guard
        .spawn_batch_command(&mut command)
        .expect("spawn child");
    record_child_process_group(registry.as_ref(), &child);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(100));
        unsafe {
            libc::raise(libc::SIGINT);
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if child.try_wait().expect("try wait").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        child.try_wait().expect("try wait").is_some(),
        "expected SIGINT handler to terminate recorded child"
    );
    assert!(guard.interrupted());
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
