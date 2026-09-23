#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::needless_pass_by_value)]

#[cfg(all(test, target_os = "linux"))]
#[used]
#[allow(non_upper_case_globals)]
#[unsafe(link_section = ".init_array")]
static prefer_tmpfs_tmpdir_init: extern "C" fn() = {
    extern "C" fn init() {
        if std::env::var_os("TMPDIR").is_none() && std::path::Path::new("/dev/shm").is_dir() {
            unsafe { std::env::set_var("TMPDIR", "/dev/shm") };
        }
    }
    init
};

mod analyze;
mod analyze_cache;
mod analyze_parse;
mod bin_cli;
#[cfg(test)]
mod layout;
mod rules;
mod test_git;
mod test_runner;
mod viz;
mod viz_coarsen;

use crate::bin_cli::{run, set_sigpipe_default};
use kiss::rust_llvm_cov_runner::{
    KissProfrawProcessGuard, discover_repo_root, redirect_this_process, sweep_kiss_profraw_dir,
};

fn main() {
    std::process::exit(run_kiss_main());
}

#[inline(never)]
fn run_kiss_main() -> i32 {
    let t0 = std::time::Instant::now();
    set_sigpipe_default();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let repo_root = discover_repo_root(&cwd);
    let _profraw_guard = if cfg!(test) {
        None
    } else {
        let _ = redirect_this_process(&repo_root);
        let _ = sweep_kiss_profraw_dir(&repo_root);
        let _ = kiss::rust_llvm_cov_runner::sweep_orphan_default_profraw(&repo_root);
        Some(KissProfrawProcessGuard::for_current_process(&repo_root))
    };
    let emit_timing = argv_emits_cli_wall_timing(std::env::args_os());
    let exit_code = run();
    if emit_timing {
        println!("{}", format_cli_wall_timing(t0.elapsed()));
    }
    exit_code
}

fn argv_emits_cli_wall_timing<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    !args
        .into_iter()
        .any(|arg| arg.as_ref() == kiss::rust_llvm_cov_runner::TARGET_RUNNER_SHIM_SUBCOMMAND)
}

pub(crate) fn format_cli_wall_timing(d: std::time::Duration) -> String {
    if d.as_secs() >= 1 {
        format!("kiss: {:.2}s", d.as_secs_f64())
    } else {
        format!("kiss: {}ms", d.as_millis())
    }
}

pub(crate) fn is_cli_wall_timing_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("kiss: ") else {
        return false;
    };
    rest.ends_with("ms") || (rest.ends_with('s') && rest.contains('.'))
}

#[cfg(test)]
pub(crate) mod cwd_test_lock {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::sync::Mutex;

    static MUTEX: Mutex<()> = Mutex::new(());
    thread_local! {
        static DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    pub struct Guard {
        _lock: Option<std::sync::MutexGuard<'static, ()>>,
        original: Option<PathBuf>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(original) = &self.original {
                let _ = std::env::set_current_dir(original);
            }
            DEPTH.set(DEPTH.get().saturating_sub(1));
        }
    }

    pub fn lock() -> Guard {
        let lock = (DEPTH.get() == 0).then(|| {
            MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        });
        DEPTH.set(DEPTH.get() + 1);
        Guard {
            _lock: lock,
            original: std::env::current_dir().ok(),
        }
    }

    #[test]
    fn guard_restores_current_directory_during_unwind() {
        const ENV: &str = "KISS_ISOLATED_CWD_GUARD_TEST";
        if std::env::var_os(ENV).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cwd_test_lock::guard_restores_current_directory_during_unwind",
                ])
                .env(ENV, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let original = std::env::current_dir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let result = std::panic::catch_unwind(|| {
            let _guard = lock();
            std::env::set_current_dir(tmp.path()).unwrap();
            panic!("exercise panic-safe restoration");
        });
        assert!(result.is_err());
        assert_eq!(std::env::current_dir().unwrap(), original);
    }
}

#[cfg(test)]
mod run_kiss_main_test {
    use std::time::Duration;

    #[test]
    fn run_kiss_main_rules_exits_zero() {
        let _lock = super::cwd_test_lock::lock();
        assert_eq!(super::run_kiss_main(), 0);
    }

    #[test]
    fn wall_timing_format_and_predicate() {
        assert_eq!(
            super::format_cli_wall_timing(Duration::from_millis(285)),
            "kiss: 285ms"
        );
        assert_eq!(
            super::format_cli_wall_timing(Duration::from_secs(2)),
            "kiss: 2.00s"
        );
        assert!(super::is_cli_wall_timing_line("kiss: 285ms"));
        assert!(super::is_cli_wall_timing_line("kiss: 2.00s"));
        assert!(!super::is_cli_wall_timing_line("kiss test: Planning ..."));
        assert!(!super::is_cli_wall_timing_line("NO VIOLATIONS"));
        assert!(super::argv_emits_cli_wall_timing(["kiss", "rules"]));
    }

    #[test]
    fn bug_report_nextest_list_rejects_cli_wall_timing_on_target_runner() {
        let timing = super::format_cli_wall_timing(Duration::from_millis(22));
        assert_eq!(timing, "kiss: 22ms");
        assert!(
            !timing.ends_with(": test") && !timing.ends_with(": benchmark"),
            "nextest list parser rejects {timing:?}"
        );

        let argv = [
            "kiss",
            kiss::rust_llvm_cov_runner::TARGET_RUNNER_SHIM_SUBCOMMAND,
        ];
        assert!(
            !super::argv_emits_cli_wall_timing(argv),
            "target runner must not emit kiss: 22ms onto nextest list stdout"
        );

        let child = "core_sources_do_not_depend_on_python_boundary_types: test";
        let mut stdout = format!("{child}\n");
        if super::argv_emits_cli_wall_timing(argv) {
            stdout.push_str(&format!("{timing}\n"));
        }
        for line in stdout.lines().filter(|line| !line.is_empty()) {
            assert!(
                line.ends_with(": test") || line.ends_with(": benchmark"),
                "bug_report.md: nextest rejects list line {line:?} in {stdout:?}"
            );
        }
    }
}
