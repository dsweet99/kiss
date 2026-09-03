use kiss::Language;
use std::sync::Mutex;

use crate::test_runner::pipeline::split_jobs;
use crate::test_runner::status_labels::print_classified_status_line;

static PIPELINE_STDOUT: Mutex<()> = Mutex::new(());

#[test]
fn print_classified_status_line_uses_emit_test_progress() {
    let src = include_str!("status_labels.rs");
    assert!(
        src.contains("emit_test_status"),
        "status lines must go through the mutex sink"
    );
    assert!(
        !src.contains("println!(\"{line}\")"),
        "status lines must not use bare println!"
    );
}

#[test]
fn jobs_split_matches_process_cap_rule() {
    assert_eq!(split_jobs(4, true), (2, 2));
    assert_eq!(split_jobs(4, false), (4, 4));
    assert_eq!(split_jobs(1, true), (1, 1));
}

#[test]
fn spawn_language_jobs_honors_configured_jobs() {
    let jobs = include_str!("pipeline_jobs.rs");
    let share = include_str!("pipeline_job_share.rs");
    let src = include_str!("pipeline.rs");
    assert!(
        jobs.contains("share.acquire_execute(language)"),
        "execute must use its fixed share without waiting for the peer language"
    );
    assert!(
        share.contains("split_jobs(self.total, self.both)"),
        "covering and execution jobs must stay on the process cap"
    );
    assert!(
        !share.contains("while self.peer_executing"),
        "language execution must not restore a cross-language barrier"
    );
    assert!(
        !src.contains("MAX_PARALLEL_TEST_JOBS"),
        "configured num_jobs must not be silently clamped"
    );
    assert_eq!(split_jobs(48, false), (48, 48));
    assert_eq!(split_jobs(48, true), (24, 24));
}

#[cfg(unix)]
#[test]
fn covering_and_workspace_lines_appear_for_all_dry_run() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
            invocation: crate::bin_cli::args::TestInvocation::All,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            force_bad: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            ignore: &[],
            lang_filter: Some(Language::Python),
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        });
    });
    std::env::set_current_dir(old).unwrap();
    assert!(
        out.contains("kiss test: Planning ..."),
        "planning heartbeat first: {out}"
    );
    assert!(
        out.contains("kiss test: Running workspace"),
        "workspace start: {out}"
    );
    assert!(
        out.contains("kiss test: Ran workspace"),
        "workspace end: {out}"
    );
    assert!(
        out.contains("kiss test: Running covering_python"),
        "covering start: {out}"
    );
    assert!(
        out.contains("kiss test: Ran covering_python"),
        "covering end: {out}"
    );
    assert!(
        !out.contains("covering_rust"),
        "--lang python must not cover rust: {out}"
    );
}

#[cfg(unix)]
#[test]
fn lang_rust_omits_covering_python() {
    let src = include_str!("pipeline.rs");
    let jobs = include_str!("pipeline_jobs.rs");
    assert!(jobs.contains("covering_python"));
    assert!(jobs.contains("covering_rust"));
    assert!(src.contains("lang_filter != Some(Language::Rust)"));
}

#[test]
fn dry_run_prints_selectors_after_covering_joins() {
    let src = include_str!("pipeline.rs");
    let spawn = src
        .find("pipeline_jobs::spawn_language_jobs")
        .expect("spawn_language_jobs call");
    let dump = src
        .find("print_joined_dry_run(&planned")
        .expect("print_joined_dry_run call");
    assert!(
        dump > spawn,
        "dry-run selector dump must wait until covering threads join"
    );
}

#[test]
fn recap_clock_is_process_start_not_summed_phases() {
    let src = include_str!("run_logic.rs");
    assert!(
        src.contains("fn summary_total_duration(_plan_duration"),
        "recap must ignore summed plan+execute durations"
    );
}

#[test]
fn cold_init_is_decided_in_shared_prefix() {
    let src = include_str!("pipeline.rs");
    let jobs = include_str!("pipeline_jobs.rs");
    assert!(
        src.contains("cold_init: should_force_cold_initialization")
            || src.contains("let cold_init = should_force_cold_initialization"),
        "cold-init must be decided in the shared prefix"
    );
    assert!(
        jobs.contains("if prefix.cold_init"),
        "language jobs must apply prefix cold-init"
    );
}

#[cfg(unix)]
#[test]
fn rust_covering_proceeds_while_python_covering_waits() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::pipeline::{COVERING_HOOKS, CoveringHooks};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

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

    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let finished = Arc::new(AtomicBool::new(false));
    let finished_job = Arc::clone(&finished);
    let job = std::thread::spawn(move || {
        let _ = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
            invocation: crate::bin_cli::args::TestInvocation::All,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            force_bad: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            ignore: &[],
            lang_filter: None,
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        });
        finished_job.store(true, Ordering::SeqCst);
    });
    let started = Instant::now();
    while !rust_started.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let rust_ok = rust_started.load(Ordering::SeqCst);
    let still_running = !finished.load(Ordering::SeqCst);
    {
        let (lock, cvar) = &*hold;
        *lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
        cvar.notify_all();
    }
    job.join().expect("language overlap job");
    std::env::set_current_dir(old).unwrap();
    *COVERING_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = CoveringHooks {
        python: None,
        rust: None,
    };
    assert!(
        rust_ok,
        "rust covering must start while python covering is held"
    );
    assert!(
        still_running,
        "run must still be waiting on python covering"
    );
}

#[allow(dead_code)]
fn touch_print(status: kiss::rpytest_runner::TestStatus) {
    print_classified_status_line(
        status,
        "t",
        std::time::Duration::from_millis(1),
        None,
        false,
    );
}

fn run_args(
    invocation: crate::bin_cli::args::TestInvocation,
    dry_run: bool,
    lang: Option<Language>,
) -> crate::test_runner::RunTestCmdArgs<'static> {
    crate::test_runner::RunTestCmdArgs {
        invocation,
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

#[cfg(unix)]
#[test]
fn rust_execute_proceeds_while_python_covering_waits() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::pipeline::{
        COVERING_HOOKS, CoveringHooks, EXECUTE_HOOKS, ExecuteHooks, STUB_LANGUAGE_EXECUTE,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();

    let hold = Arc::new((Mutex::new(true), Condvar::new()));
    let rust_executed = Arc::new(AtomicBool::new(false));
    let hold_py = Arc::clone(&hold);
    let rust_flag = Arc::clone(&rust_executed);
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
        rust: None,
    };
    *EXECUTE_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = ExecuteHooks {
        python: None,
        rust: Some(Arc::new(move || {
            rust_flag.store(true, Ordering::SeqCst);
        })),
    };
    STUB_LANGUAGE_EXECUTE.store(true, Ordering::SeqCst);

    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let finished = Arc::new(AtomicBool::new(false));
    let finished_job = Arc::clone(&finished);
    let job = std::thread::spawn(move || {
        let _ = crate::test_runner::run_test(run_args(
            crate::bin_cli::args::TestInvocation::All,
            false,
            None,
        ));
        finished_job.store(true, Ordering::SeqCst);
    });
    let started = Instant::now();
    while !rust_executed.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let rust_ok = rust_executed.load(Ordering::SeqCst);
    let still_running = !finished.load(Ordering::SeqCst);
    {
        let (lock, cvar) = &*hold;
        *lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
        cvar.notify_all();
    }
    job.join().expect("live overlap job");
    std::env::set_current_dir(old).unwrap();
    STUB_LANGUAGE_EXECUTE.store(false, Ordering::SeqCst);
    *COVERING_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = CoveringHooks {
        python: None,
        rust: None,
    };
    *EXECUTE_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = ExecuteHooks {
        python: None,
        rust: None,
    };
    assert!(
        rust_ok,
        "rust execute must start while python covering is held"
    );
    assert!(
        still_running,
        "run must still be waiting on python covering"
    );
}

#[cfg(unix)]
#[test]
fn peer_does_not_execute_after_rust_covering_failure() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::pipeline::{
        COVERING_HOOKS, CoveringHooks, EXECUTE_HOOKS, ExecuteHooks, STUB_LANGUAGE_EXECUTE,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();

    let hold = Arc::new((Mutex::new(true), Condvar::new()));
    let rust_covering = Arc::new(AtomicBool::new(false));
    let python_executed = Arc::new(AtomicBool::new(false));
    let hold_py = Arc::clone(&hold);
    let rust_flag = Arc::clone(&rust_covering);
    let python_flag = Arc::clone(&python_executed);
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
    *EXECUTE_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = ExecuteHooks {
        python: Some(Arc::new(move || {
            python_flag.store(true, Ordering::SeqCst);
        })),
        rust: None,
    };
    STUB_LANGUAGE_EXECUTE.store(true, Ordering::SeqCst);
    crate::test_runner::pipeline::set_fail_covering(Some(Language::Rust));

    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let job = std::thread::spawn(|| {
        crate::test_runner::run_test(run_args(
            crate::bin_cli::args::TestInvocation::All,
            false,
            None,
        ))
    });
    let started = Instant::now();
    while !rust_covering.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(50));
    {
        let (lock, cvar) = &*hold;
        *lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
        cvar.notify_all();
    }
    let result = job.join().expect("pipeline job");
    std::env::set_current_dir(old).unwrap();
    crate::test_runner::pipeline::set_fail_covering(None);
    STUB_LANGUAGE_EXECUTE.store(false, Ordering::SeqCst);
    *COVERING_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = CoveringHooks {
        python: None,
        rust: None,
    };
    *EXECUTE_HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = ExecuteHooks {
        python: None,
        rust: None,
    };
    assert_ne!(result, 0);
    assert!(
        !python_executed.load(Ordering::SeqCst),
        "peer must not enter execute after a recorded covering failure"
    );
}

#[cfg(unix)]
#[test]
fn workspace_span_completes_before_covering_error() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    crate::test_runner::pipeline::set_fail_covering(Some(Language::Python));
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = crate::test_runner::run_test(run_args(
            crate::bin_cli::args::TestInvocation::All,
            true,
            Some(Language::Python),
        ));
    });
    std::env::set_current_dir(old).unwrap();
    crate::test_runner::pipeline::set_fail_covering(None);
    let workspace_start = out
        .find("kiss test: Running workspace")
        .expect("workspace start");
    let workspace_end = out.find("kiss test: Ran workspace").expect("workspace end");
    let covering = out
        .find("kiss test: Running covering_python")
        .expect("covering start");
    assert!(
        workspace_start < workspace_end && workspace_end < covering,
        "covering error must follow completed workspace span: {out}"
    );
}

#[test]
fn force_all_runs_in_language_thread_after_covering() {
    let src = include_str!("pipeline_jobs.rs");
    let cover = src
        .find("let mut planned = cover_language")
        .expect("cover_language");
    let ran = src.find("Ran {covering_name}").expect("Ran covering");
    let force = src
        .find("apply_force_all_population(a, &mut planned)")
        .expect("force_all");
    assert!(
        cover < ran && ran < force,
        "force_all must run in the language thread after covering Ran"
    );
}

#[test]
fn covering_population_overlaps_list_build() {
    let src = include_str!("coverage_decision/engine.rs");
    assert!(
        src.contains("overlap_with_discover(|| planner.discover_universe())"),
        "rust plan_population must overlap list-build with discover_universe"
    );
}

#[cfg(unix)]
#[test]
fn recap_wall_time_tracks_process_clock() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::pipeline::STUB_LANGUAGE_EXECUTE;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    STUB_LANGUAGE_EXECUTE.store(true, Ordering::SeqCst);
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let started = Instant::now();
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = crate::test_runner::run_test(run_args(
            crate::bin_cli::args::TestInvocation::All,
            false,
            Some(Language::Python),
        ));
    });
    let wall = started.elapsed();
    std::env::set_current_dir(old).unwrap();
    STUB_LANGUAGE_EXECUTE.store(false, Ordering::SeqCst);
    let recap = out
        .lines()
        .find(|line| line.contains(" total · "))
        .and_then(|line| {
            line.split(" · ")
                .find_map(|part| part.strip_suffix("s total")?.parse::<f64>().ok())
        });
    if let Some(seconds) = recap {
        let recap_dur = Duration::from_secs_f64(seconds.max(0.0));
        assert!(
            recap_dur <= wall + Duration::from_millis(250),
            "recap {recap_dur:?} must track wall {wall:?}: {out}"
        );
    }
}
