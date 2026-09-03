use std::process::{Command, Stdio};
use std::time::Duration;

use super::{BatchProcessTreeGuard, record_child_process_group};

fn signal_test_guard() -> std::sync::MutexGuard<'static, ()> {
    super::signal_test_guard()
}

fn run_signal_body_in_isolated_process(test_name: &str) -> bool {
    const ENV: &str = "KISS_ISOLATED_SIGNAL_TEST";
    if std::env::var(ENV).as_deref() == Ok(test_name) {
        return false;
    }
    let full_name = format!(
        "rust_llvm_cov_runner::execute_or_reuse::batch_process_tree::sigint_tests::{test_name}"
    );
    let status = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", &full_name])
        .env(ENV, test_name)
        .status()
        .expect("spawn isolated signal test");
    assert!(status.success(), "isolated signal test failed: {status}");
    true
}

#[cfg(unix)]
#[test]
fn batch_process_tree_guard_sigint_handler_escalates_signal_ignoring_child() {
    if run_signal_body_in_isolated_process(
        "batch_process_tree_guard_sigint_handler_escalates_signal_ignoring_child",
    ) {
        return;
    }
    let _serial = signal_test_guard();
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("trap '' TERM INT; sleep 60");
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
        "expected SIGINT handler to escalate to recorded child"
    );
    assert!(guard.interrupted());
}

#[test]
fn sigint_handler_install_and_clear_direct() {
    let _serial = signal_test_guard();
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    super::install_sigint_handler(
        Arc::new(super::ProcessTreeRegistry::default()),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("install sigint handler");
    super::clear_sigint_handler();
}

#[test]
fn register_batch_scope_sigint_and_clear_batch_scope_sigint_direct() {
    let _serial = signal_test_guard();
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let registry = Arc::new(super::ProcessTreeRegistry::default());
    let interrupted = Arc::new(AtomicBool::new(false));
    super::register_batch_scope_sigint(Arc::clone(&registry), Arc::clone(&interrupted))
        .expect("register batch scope sigint");
    assert!(!super::batch_scope_interrupted());
    super::clear_batch_scope_sigint();
    assert!(!super::batch_scope_interrupted());
}

#[cfg(target_os = "linux")]
#[test]
fn install_child_subreaper_direct() {
    let _serial = signal_test_guard();
    super::install_child_subreaper().expect("install child subreaper");
}

#[cfg(unix)]
#[test]
fn handle_sigint_is_installed_with_scope_guard() {
    let _serial = signal_test_guard();
    let _scope = super::BatchScopeInterruptGuard::install().expect("install scope guard");
    let _handler: extern "C" fn(i32) = super::handle_sigint;
}

#[cfg(target_os = "linux")]
#[test]
fn linux_subreaper_installs_via_process_tree_guard() {
    let _serial = signal_test_guard();
    let _guard = BatchProcessTreeGuard::install().expect("install guard");
}

#[test]
fn clear_sigint_handler_is_safe_before_install() {
    let _serial = signal_test_guard();
    super::clear_sigint_handler();
}

#[test]
fn batch_scope_interrupted_returns_false_without_active_scope() {
    let _serial = signal_test_guard();
    assert!(!super::batch_scope_interrupted());
}

#[test]
fn batch_scope_interrupt_guard_reinstalls_after_drop() {
    let _serial = signal_test_guard();
    {
        let _scope = super::BatchScopeInterruptGuard::install().expect("install scope guard");
        assert!(!super::batch_scope_interrupted());
    }
    assert!(!super::batch_scope_interrupted());
    let _scope = super::BatchScopeInterruptGuard::install().expect("reinstall scope guard");
    assert!(!super::batch_scope_interrupted());
}

#[test]
fn scope_shared_process_tree_guard_drop_does_not_mark_scope_interrupted() {
    let _serial = signal_test_guard();
    let _scope = super::BatchScopeInterruptGuard::install().expect("install scope guard");
    {
        let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 0.05");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let mut child = guard
            .spawn_batch_command(&mut command)
            .expect("spawn child");
        record_child_process_group(guard.registry().as_ref(), &child);
        child.wait().expect("wait child");
    }
    assert!(
        !super::batch_scope_interrupted(),
        "normal subprocess teardown must not mark the scope interrupted"
    );
}

#[cfg(unix)]
#[test]
fn batch_scope_sigint_escalates_signal_ignoring_child() {
    if run_signal_body_in_isolated_process("batch_scope_sigint_escalates_signal_ignoring_child") {
        return;
    }
    let _serial = signal_test_guard();
    let _scope = super::BatchScopeInterruptGuard::install().expect("install scope guard");
    let guard = BatchProcessTreeGuard::install().expect("install process tree guard");
    let registry = guard.registry();
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg("trap '' TERM INT; sleep 60");
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
        "expected scope SIGINT handler to escalate to recorded child"
    );
    assert!(super::batch_scope_interrupted());
    assert!(guard.interrupted());
}

#[cfg(unix)]
#[test]
fn batch_scope_interrupt_guard_marks_scope_interrupted_on_sigint() {
    if run_signal_body_in_isolated_process(
        "batch_scope_interrupt_guard_marks_scope_interrupted_on_sigint",
    ) {
        return;
    }
    let _serial = signal_test_guard();
    let _scope = super::BatchScopeInterruptGuard::install().expect("install scope guard");
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
    assert!(super::batch_scope_interrupted());
    assert!(guard.interrupted());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
#[test]
fn batch_process_tree_guard_sigint_handler_sets_interrupted_without_children() {
    if run_signal_body_in_isolated_process(
        "batch_process_tree_guard_sigint_handler_sets_interrupted_without_children",
    ) {
        return;
    }
    let _serial = signal_test_guard();
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
    if run_signal_body_in_isolated_process(
        "batch_process_tree_guard_sigint_handler_terminates_recorded_child",
    ) {
        return;
    }
    let _serial = signal_test_guard();
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
