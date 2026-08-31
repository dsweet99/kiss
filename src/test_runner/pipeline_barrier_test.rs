use kiss::Language;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar};
use std::time::{Duration, Instant};

use crate::test_runner::pipeline::{
    COVERING_HOOKS, CoveringHooks, set_blocked_covering_language, unpark_blocked_covering,
};

static BARRIER_STDOUT: Mutex<()> = Mutex::new(());

fn run_args(
    dry_run: bool,
    lang: Option<Language>,
) -> crate::test_runner::RunTestCmdArgs<'static> {
    crate::test_runner::RunTestCmdArgs {
        invocation: crate::bin_cli::args::TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: lang,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

fn wait_flag(flag: &AtomicBool) {
    let started = Instant::now();
    while !flag.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn clear_covering_hooks() {
    set_blocked_covering_language(None);
    *COVERING_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = CoveringHooks {
        python: None,
        rust: None,
    };
}

fn no_selector_dump(out: &str) -> bool {
    !out.contains("RUST SELECTOR")
        && !out.contains("RUST BATCH")
        && !out.contains("python -m pytest")
}

#[test]
fn status_printers_use_emit_test_progress() {
    let rslip = include_str!("lang_python/rslip.rs");
    assert!(rslip.contains("emit_test_progress"));
    assert!(!rslip.contains("writeln!(stdout"));
    assert!(!rslip.contains("writeln!(out,"));
    assert!(!rslip.contains("write_all(body"));
    let witness = include_str!("lang_iface/witness.rs");
    assert!(witness.contains("emit_test_progress(&format!(\"{label} (cached)"));
    assert!(!witness.contains("println!(\"{label} (cached)"));
    let dry = include_str!("run_logic/language_executor.rs");
    assert!(dry.contains("emit_test_progress(&line)"));
}

#[cfg(unix)]
#[test]
fn covering_rust_running_appears_before_blocked_planner_returns() {
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();
    let reached = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&reached);
    *COVERING_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = CoveringHooks {
        python: None,
        rust: Some(Arc::new(move || {
            flag.store(true, Ordering::SeqCst);
        })),
    };
    set_blocked_covering_language(Some(Language::Rust));
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = BARRIER_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let finished = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&finished);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let job = std::thread::spawn(move || {
            let _ = crate::test_runner::run_test(run_args(true, Some(Language::Rust)));
            done.store(true, Ordering::SeqCst);
        });
        wait_flag(&reached);
        let blocked = !finished.load(Ordering::SeqCst);
        unpark_blocked_covering();
        job.join().expect("blocked covering job");
        assert!(blocked, "planner must still be parked after Running");
    });
    std::env::set_current_dir(old).unwrap();
    clear_covering_hooks();
    let running = out
        .find("kiss test: Running covering_rust")
        .expect("Running covering_rust");
    let ran = out.find("kiss test: Ran covering_rust").expect("Ran covering");
    assert!(running < ran, "Running must precede Ran: {out}");
    assert!(
        out.contains("kiss test: Ran covering_rust") && out.contains("ms"),
        "Ran covering_rust must include ms: {out}"
    );
}

#[cfg(unix)]
#[test]
fn dry_run_omits_rust_selectors_until_python_covering_finishes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();
    let hold = Arc::new((Mutex::new(true), Condvar::new()));
    let rust_started = Arc::new(AtomicBool::new(false));
    let hold_py = Arc::clone(&hold);
    let rust_flag = Arc::clone(&rust_started);
    *COVERING_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = CoveringHooks {
        python: Some(Arc::new(move || {
            let (lock, cvar) = &*hold_py;
            let mut waiting = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while *waiting {
                waiting = cvar
                    .wait(waiting)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        })),
        rust: Some(Arc::new(move || {
            rust_flag.store(true, Ordering::SeqCst);
        })),
    };
    let log = tmp.path().join("stdout.log");
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = BARRIER_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    with_stdout_file(&log, || {
        let job = std::thread::spawn(move || {
            let _ = crate::test_runner::run_test(run_args(true, None));
        });
        wait_flag(&rust_started);
        wait_log_contains(&log, "kiss test: Ran covering_rust");
        let held = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            no_selector_dump(&held),
            "dump must wait for python covering: {held}"
        );
        {
            let (lock, cvar) = &*hold;
            *lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
            cvar.notify_all();
        }
        job.join().expect("dry-run barrier job");
    });
    std::env::set_current_dir(old).unwrap();
    clear_covering_hooks();
}

#[cfg(unix)]
fn wait_log_contains(path: &std::path::Path, needle: &str) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        if std::fs::read_to_string(path)
            .unwrap_or_default()
            .contains(needle)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {needle} in {}",
        std::fs::read_to_string(path).unwrap_or_default()
    );
}

#[cfg(unix)]
fn with_stdout_file(path: &std::path::Path, f: impl FnOnce()) {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    let file = std::fs::File::create(path).unwrap();
    let fd = file.as_raw_fd();
    let old = unsafe { libc::dup(libc::STDOUT_FILENO) };
    assert!(old >= 0);
    assert_eq!(unsafe { libc::dup2(fd, libc::STDOUT_FILENO) }, libc::STDOUT_FILENO);
    f();
    let _ = std::io::stdout().flush();
    assert_eq!(
        unsafe { libc::dup2(old, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    unsafe {
        libc::close(old);
    }
}
