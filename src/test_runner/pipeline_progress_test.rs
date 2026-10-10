use kiss::Language;
use std::sync::Mutex;

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
fn spawn_language_jobs_honors_configured_jobs() {
    let jobs = include_str!("pipeline_jobs.rs");
    let share = include_str!("pipeline_job_share.rs");
    let src = include_str!("pipeline.rs");
    assert!(
        jobs.contains("for language in crate::test_runner::lang_registry::languages()")
            && jobs.contains("expect_language_remaining(language)"),
        "spawned languages must be expected so remaining=0 waits for the peer"
    );
    assert!(
        jobs.contains("share.acquire_execute(language)"),
        "execute must use its fixed share without waiting for the peer language"
    );
    assert!(
        share.contains("jobs: self.total"),
        "execute must use the full configured job budget per language"
    );
    assert!(
        !share.contains("while self.peer_executing"),
        "language execution must not restore a cross-language barrier"
    );
    assert!(
        !src.contains("MAX_PARALLEL_TEST_JOBS"),
        "configured num_jobs must not be silently clamped"
    );
}

#[cfg(unix)]
#[test]
fn selecting_and_workspace_lines_appear_for_all_dry_run() {
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
        let _ = crate::test_runner::pipeline::run_overlapped_test(
            &crate::test_runner::RunTestCmdArgs {
                doubles: None,
                invocation: crate::bin_cli::args::TestInvocation::All,
                target_request: crate::test_runner::target_request::workspace_request(
                    Some(Language::Python),
                    &[],
                ),
                main_branch_cli: None,
                base_branch_cli: None,
                dry_run: true,
                force_rerun: false,
                metrics: false,
                jobs: 1,
                extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
                config_main_branch: None,
                gate_config: kiss::GateConfig::default(),
            },
            std::time::Instant::now(),
        );
    });
    std::env::set_current_dir(old).unwrap();
    assert!(
        out.contains("kiss test: Running workspace"),
        "workspace start: {out}"
    );
    assert!(
        out.contains("kiss test: Ran workspace"),
        "workspace end: {out}"
    );
    assert!(
        out.contains("kiss test: Running select_python"),
        "selecting start: {out}"
    );
    assert!(
        out.contains("kiss test: Ran select_python"),
        "selecting end: {out}"
    );
    assert!(
        !out.contains("select_rust"),
        "--lang python must not cover rust: {out}"
    );
}

#[cfg(unix)]
#[test]
fn lang_rust_omits_selecting_python() {
    let src = include_str!("pipeline.rs");
    let jobs = include_str!("pipeline_jobs.rs");
    assert!(jobs.contains("format!(\"select_{}\", language.label())"));
    assert_eq!(
        format!("select_{}", Language::Python.label()),
        "select_python"
    );
    assert_eq!(format!("select_{}", Language::Rust.label()), "select_rust");
    assert!(src.contains("language.allowed_by(a.lang_filter())"));
    assert!(!Language::Python.allowed_by(Some(Language::Rust)));
    assert!(Language::Rust.allowed_by(Some(Language::Rust)));
}

#[cfg(unix)]
#[test]
fn empty_commit_dry_run_prints_no_selected_tests() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("README"), "x\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        assert!(
            crate::test_runner::test_mode_fixtures::git_in(tmp.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let code = crate::test_runner::pipeline::run_overlapped_test(
            &crate::test_runner::RunTestCmdArgs {
                doubles: None,
                invocation: crate::bin_cli::args::TestInvocation::Commit,
                target_request: crate::test_runner::target_request::request_from_focus(
                    crate::test_runner::target_request::TargetFocus::Git(
                        crate::test_runner::target_request::GitFocus::Commit,
                    ),
                    None,
                    &[],
                ),
                main_branch_cli: None,
                base_branch_cli: None,
                dry_run: true,
                force_rerun: false,
                metrics: false,
                jobs: 1,
                extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
                config_main_branch: None,
                gate_config: kiss::GateConfig::default(),
            },
            std::time::Instant::now(),
        )
        .unwrap_or(1);
        assert_eq!(code, 0);
    });
    std::env::set_current_dir(old).unwrap();
    assert!(
        out.contains("NO SELECTED TESTS"),
        "empty selector list must match finish_no_work: {out}"
    );
    assert!(!out.contains("kiss test: Planning"), "{out}");
    assert!(!out.contains("kiss test: plan complete="), "{out}");
    assert!(!out.contains("kiss test: plan execute="), "{out}");
    assert!(!out.contains("kiss test: graph repair"), "{out}");
    assert!(!out.contains("kiss test: kernel parse="), "{out}");
}

#[test]
fn dry_run_prints_selectors_after_selecting_joins() {
    let src = include_str!("pipeline.rs");
    let spawn = src
        .find("pipeline_jobs::spawn_language_jobs")
        .expect("spawn_language_jobs call");
    let dump = src
        .find("print_joined_dry_run(&planned")
        .expect("print_joined_dry_run call");
    assert!(
        dump > spawn,
        "dry-run selector dump must wait until selecting threads join"
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
fn rust_selecting_proceeds_while_python_selecting_waits() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::language_keyed::LanguageKeyed;
    use crate::test_runner::pipeline::PipelineDoubles;
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
    let selecting: LanguageKeyed<Option<Arc<dyn Fn() + Send + Sync>>> = LanguageKeyed {
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
        let _ = crate::test_runner::pipeline::run_overlapped_test(
            &crate::test_runner::RunTestCmdArgs {
                doubles: Some(Arc::new(PipelineDoubles {
                    selecting,
                    ..PipelineDoubles::default()
                })),
                invocation: crate::bin_cli::args::TestInvocation::All,
                target_request: crate::test_runner::target_request::workspace_request(None, &[]),
                main_branch_cli: None,
                base_branch_cli: None,
                dry_run: true,
                force_rerun: false,
                metrics: false,
                jobs: 1,
                extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
                config_main_branch: None,
                gate_config: kiss::GateConfig::default(),
            },
            Instant::now(),
        );
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
    assert!(
        rust_ok,
        "rust selecting must start while python selecting is held"
    );
    assert!(
        still_running,
        "run must still be waiting on python selecting"
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

fn with_doubles(
    mut args: crate::test_runner::RunTestCmdArgs<'static>,
    doubles: crate::test_runner::pipeline::PipelineDoubles,
) -> crate::test_runner::RunTestCmdArgs<'static> {
    args.doubles = Some(std::sync::Arc::new(doubles));
    args
}

fn run_args(
    invocation: crate::bin_cli::args::TestInvocation,
    dry_run: bool,
    lang: Option<Language>,
) -> crate::test_runner::RunTestCmdArgs<'static> {
    crate::test_runner::RunTestCmdArgs {
        doubles: None,
        invocation: invocation.clone(),
        target_request: crate::test_runner::target_request::request_from_invocation(
            &invocation,
            None,
            None,
            None,
            lang,
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

#[cfg(unix)]
#[test]
fn rust_execute_proceeds_while_python_selecting_waits() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::language_keyed::LanguageKeyed;
    use crate::test_runner::pipeline::PipelineDoubles;
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
    let selecting: LanguageKeyed<Option<Arc<dyn Fn() + Send + Sync>>> = LanguageKeyed {
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
    let execute: LanguageKeyed<Option<Arc<dyn Fn() + Send + Sync>>> = LanguageKeyed {
        python: None,
        rust: Some(Arc::new(move || {
            rust_flag.store(true, Ordering::SeqCst);
        })),
    };
    let doubles = PipelineDoubles {
        selecting,
        execute,
        stub_execute: true,
        ..PipelineDoubles::default()
    };

    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let finished = Arc::new(AtomicBool::new(false));
    let finished_job = Arc::clone(&finished);
    let job = std::thread::spawn(move || {
        let _ = crate::test_runner::pipeline::run_overlapped_test(
            &with_doubles(
                run_args(crate::bin_cli::args::TestInvocation::All, false, None),
                doubles,
            ),
            Instant::now(),
        );
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
    assert!(
        rust_ok,
        "rust execute must start while python selecting is held"
    );
    assert!(
        still_running,
        "run must still be waiting on python selecting"
    );
}

#[cfg(unix)]
#[test]
fn peer_does_not_execute_after_rust_selecting_failure() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::language_keyed::LanguageKeyed;
    use crate::test_runner::pipeline::PipelineDoubles;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();

    let hold = Arc::new((Mutex::new(true), Condvar::new()));
    let rust_selecting = Arc::new(AtomicBool::new(false));
    let python_executed = Arc::new(AtomicBool::new(false));
    let hold_py = Arc::clone(&hold);
    let rust_flag = Arc::clone(&rust_selecting);
    let python_flag = Arc::clone(&python_executed);
    let selecting: LanguageKeyed<Option<Arc<dyn Fn() + Send + Sync>>> = LanguageKeyed {
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
    let execute: LanguageKeyed<Option<Arc<dyn Fn() + Send + Sync>>> = LanguageKeyed {
        python: Some(Arc::new(move || {
            python_flag.store(true, Ordering::SeqCst);
        })),
        rust: None,
    };
    let doubles = PipelineDoubles {
        selecting,
        execute,
        stub_execute: true,
        fail_selecting: Some(Language::Rust),
        ..PipelineDoubles::default()
    };

    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let job = std::thread::spawn(move || {
        crate::test_runner::pipeline::run_overlapped_test(
            &with_doubles(
                run_args(crate::bin_cli::args::TestInvocation::All, false, None),
                doubles,
            ),
            Instant::now(),
        )
    });
    let started = Instant::now();
    while !rust_selecting.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(15) {
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
    assert!(
        rust_selecting.load(Ordering::SeqCst),
        "rust selecting must start while python selecting is held"
    );
    assert!(result.is_err(), "rust selecting failure must fail the run");
    assert!(
        !python_executed.load(Ordering::SeqCst),
        "peer must not enter execute after a recorded selecting failure"
    );
}

#[cfg(unix)]
#[test]
fn workspace_span_completes_before_selecting_error() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    let doubles = crate::test_runner::pipeline::PipelineDoubles {
        fail_selecting: Some(Language::Python),
        ..crate::test_runner::pipeline::PipelineDoubles::default()
    };
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = crate::test_runner::pipeline::run_overlapped_test(
            &with_doubles(
                run_args(
                    crate::bin_cli::args::TestInvocation::All,
                    true,
                    Some(Language::Python),
                ),
                doubles,
            ),
            std::time::Instant::now(),
        );
    });
    std::env::set_current_dir(old).unwrap();
    let workspace_start = out
        .find("kiss test: Running workspace")
        .expect("workspace start");
    let workspace_end = out.find("kiss test: Ran workspace").expect("workspace end");
    let selecting = out
        .find("kiss test: Running select_python")
        .expect("selecting start");
    assert!(
        workspace_start < workspace_end && workspace_end < selecting,
        "selecting error must follow completed workspace span: {out}"
    );
}

#[test]
fn force_all_runs_in_language_thread_after_selecting() {
    let src = include_str!("pipeline_jobs.rs");
    let cover = src
        .find("let mut planned = select_language")
        .expect("select_language");
    let ran = src.find("Ran {selecting_name}").expect("Ran selecting");
    let force = src
        .find("apply_force_all_population(a, &mut planned)")
        .expect("force_all");
    assert!(
        cover < ran && ran < force,
        "force_all must run in the language thread after selecting Ran"
    );
}

#[cfg(unix)]
#[test]
fn recap_wall_time_tracks_process_clock() {
    let _cwd = crate::cwd_test_lock::lock();
    use crate::test_runner::pipeline::PipelineDoubles;
    use std::time::{Duration, Instant};

    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    std::fs::write(tmp.path().join("lib.py"), "x = 1\n").unwrap();
    let doubles = PipelineDoubles {
        stub_execute: true,
        ..PipelineDoubles::default()
    };
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let _stdout = PIPELINE_STDOUT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let started = Instant::now();
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = crate::test_runner::run_test(with_doubles(
            run_args(
                crate::bin_cli::args::TestInvocation::All,
                false,
                Some(Language::Python),
            ),
            doubles,
        ));
    });
    let wall = started.elapsed();
    std::env::set_current_dir(old).unwrap();
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
