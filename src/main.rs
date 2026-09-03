#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::needless_pass_by_value)]

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
        Some(KissProfrawProcessGuard::for_current_process(&repo_root))
    };
    let exit_code = run();
    let d = t0.elapsed();
    if d.as_secs() >= 1 {
        eprintln!("kiss: {:.2}s", d.as_secs_f64());
    } else {
        eprintln!("kiss: {}ms", d.as_millis());
    }
    exit_code
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
    #[test]
    fn run_kiss_main_rules_exits_zero() {
        let _lock = super::cwd_test_lock::lock();
        assert_eq!(super::run_kiss_main(), 0);
    }
}
