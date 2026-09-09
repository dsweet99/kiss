use std::cell::Cell;
use std::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessBudgetBreach {
    pub live: usize,
    pub cap: usize,
}

const LLVM_COV_NEXTEST_SLACK: usize = 4;

thread_local! {
    static LIVE_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(crate) fn llvm_cov_nextest_process_cap() -> usize {
    1 + crate::test_section_config::TestSectionConfig::load().num_jobs_llvm_cov
        + LLVM_COV_NEXTEST_SLACK
}

pub(crate) fn check_llvm_cov_nextest_budget() -> Result<(), ProcessBudgetBreach> {
    let live = current_llvm_cov_nextest_count();
    let cap = llvm_cov_nextest_process_cap();
    if live > cap {
        return Err(ProcessBudgetBreach { live, cap });
    }
    Ok(())
}

fn current_llvm_cov_nextest_count() -> usize {
    if let Some(live) = LIVE_OVERRIDE.with(Cell::get) {
        return live;
    }
    count_llvm_cov_nextest_processes()
}

fn count_llvm_cov_nextest_processes() -> usize {
    let Ok(proc_dir) = fs::read_dir("/proc") else {
        return 0;
    };
    let self_pid = std::process::id();
    let self_uid = current_uid();
    proc_dir
        .flatten()
        .filter(|entry| is_own_llvm_cov_nextest(&entry.file_name(), self_pid, self_uid))
        .count()
}

fn is_own_llvm_cov_nextest(name: &std::ffi::OsString, self_pid: u32, self_uid: u32) -> bool {
    match name.to_string_lossy().parse::<u32>() {
        Ok(pid) if pid > 1 && pid != self_pid => {}
        _ => return false,
    }
    let proc_path = std::path::Path::new("/proc").join(name);
    if process_uid(&proc_path) != Some(self_uid) {
        return false;
    }
    let Ok(raw) = fs::read(proc_path.join("cmdline")) else {
        return false;
    };
    cmdline_is_llvm_cov_nextest(&raw)
}

fn cmdline_is_llvm_cov_nextest(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    text.contains("llvm-cov") && text.contains("nextest")
}

fn process_uid(proc_path: &std::path::Path) -> Option<u32> {
    let status = fs::read_to_string(proc_path.join("status")).ok()?;
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("Uid:") else {
            continue;
        };
        return rest.split_whitespace().next()?.parse().ok();
    }
    None
}

pub(crate) fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

#[cfg(test)]
pub(crate) struct ProcessCountOverrideGuard {
    previous: Option<usize>,
}

#[cfg(test)]
impl ProcessCountOverrideGuard {
    pub(crate) fn enter(live: Option<usize>) -> Self {
        let previous = LIVE_OVERRIDE.with(|slot| {
            let previous = slot.get();
            slot.set(live);
            previous
        });
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for ProcessCountOverrideGuard {
    fn drop(&mut self) {
        LIVE_OVERRIDE.with(|slot| slot.set(self.previous));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProcessBudgetBreach, ProcessCountOverrideGuard, check_llvm_cov_nextest_budget,
        cmdline_is_llvm_cov_nextest, llvm_cov_nextest_process_cap,
    };

    #[test]
    fn cmdline_counts_nextest_parents_not_rustc_wrappers() {
        assert!(cmdline_is_llvm_cov_nextest(
            b"cargo-llvm-cov\0llvm-cov\0nextest\0--no-report"
        ));
        assert!(cmdline_is_llvm_cov_nextest(b"cargo\0llvm-cov\0nextest"));
        assert!(!cmdline_is_llvm_cov_nextest(
            b"/home/u/.cargo/bin/cargo-llvm-cov\0rustc\0--crate-name\0foo"
        ));
    }

    #[test]
    fn process_budget_is_hard_error_above_cap() {
        let cap = llvm_cov_nextest_process_cap();
        let _live = ProcessCountOverrideGuard::enter(Some(cap + 1));
        let err = check_llvm_cov_nextest_budget().unwrap_err();
        assert_eq!(
            err,
            ProcessBudgetBreach {
                live: cap + 1,
                cap
            }
        );
    }

    #[test]
    fn process_budget_allows_cap_and_below() {
        let cap = llvm_cov_nextest_process_cap();
        let _at_cap = ProcessCountOverrideGuard::enter(Some(cap));
        check_llvm_cov_nextest_budget().unwrap();
        drop(_at_cap);
        let _below = ProcessCountOverrideGuard::enter(Some(0));
        check_llvm_cov_nextest_budget().unwrap();
    }

    #[test]
    fn process_cap_uses_named_llvm_cov_width_not_num_jobs() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "[test]\nnum_jobs = 48\nnum_jobs_llvm_cov = 3\n").unwrap();
        let _guard = crate::config::ConfigPathOverrideGuard::enter(Some(tmp.path()));
        assert_eq!(llvm_cov_nextest_process_cap(), 1 + 3 + 4);
        assert_ne!(llvm_cov_nextest_process_cap(), 48);
    }
}
