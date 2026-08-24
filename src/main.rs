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
use rust_llvm_cov_runner::{
    KissProfrawProcessGuard, discover_repo_root, redirect_this_process, sweep_kiss_profraw_dir,
};

#[doc = "kiss-coverage-off"]
fn main() {
    std::process::exit(run_kiss_main());
}

#[doc = "kiss-coverage-off"]
#[inline(never)]
fn run_kiss_main() -> i32 {
    let t0 = std::time::Instant::now();
    set_sigpipe_default();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let repo_root = discover_repo_root(&cwd);
    let _ = redirect_this_process(&repo_root);
    let _ = sweep_kiss_profraw_dir(&repo_root);
    let _profraw_guard = KissProfrawProcessGuard::for_current_process(&repo_root);
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
    use std::sync::Mutex;

    static MUTEX: Mutex<()> = Mutex::new(());

    pub fn lock() -> std::sync::MutexGuard<'static, ()> {
        MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
