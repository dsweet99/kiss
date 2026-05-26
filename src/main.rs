#![allow(clippy::redundant_pub_crate)]
// CLI/analyze use owned "context" structs at API boundaries; pedantic prefers references everywhere.
#![allow(clippy::needless_pass_by_value)]

mod analyze;
mod analyze_cache;
mod analyze_parse;
mod test_git;
mod test_runner;
mod bin_cli;
#[cfg(test)]
mod layout;
mod rules;
mod test_discovery;
mod viz;
mod viz_coarsen;

fn main() {
    std::process::exit(crate::bin_cli::kiss_main_with_timing());
}

#[cfg(test)]
pub(crate) mod cwd_test_lock {
    use std::sync::Mutex;

    static MUTEX: Mutex<()> = Mutex::new(());

    pub fn lock() -> std::sync::MutexGuard<'static, ()> {
        MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
