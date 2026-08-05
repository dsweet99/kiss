//! Native filesystem watch session for `kiss test --watch`.

mod event_source;
mod filter;
mod roots;
mod session;
mod settle;

#[allow(unused_imports)] // re-exported for sibling modules and tests
pub(crate) use event_source::{NativeWatchEventSource, NormalizedWatchEvent, WatchEventSource};
#[allow(unused_imports)]
pub(crate) use filter::WatchPathFilter;
#[allow(unused_imports)]
pub(crate) use roots::resolve_watch_registrations;
pub(crate) use session::run_test_watch;
#[allow(unused_imports)]
pub(crate) use session::run_watch_loop;
#[allow(unused_imports)]
pub(crate) use settle::{PathSignature, SettleMachine, SettlePoll};

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::bin_cli::args::TestInvocation;

use event_source::NormalizedWatchEvent as Ev;
use filter::WatchPathFilter as Filter;
use settle::{PathSignature as Sig, SettleMachine as Machine};

pub(crate) fn invocation_label(invocation: &TestInvocation) -> String {
    match invocation {
        TestInvocation::Commit => "commit".into(),
        TestInvocation::Base => "base".into(),
        TestInvocation::Main => "main".into(),
        TestInvocation::All => ".".into(),
        TestInvocation::Targets(targets) => targets.join(" "),
    }
}

pub(crate) fn print_cycle_summary(paths: &[PathBuf]) {
    let mut sorted: Vec<_> = paths.iter().map(|p| p.display().to_string()).collect();
    sorted.sort();
    let shown: Vec<_> = sorted.iter().take(10).cloned().collect();
    let extra = sorted.len().saturating_sub(shown.len());
    if extra == 0 {
        println!(
            "kiss test --watch: {} change(s): {}",
            sorted.len(),
            shown.join(", ")
        );
    } else {
        println!(
            "kiss test --watch: {} change(s): {} (+{} more)",
            sorted.len(),
            shown.join(", "),
            extra
        );
    }
}

pub(crate) fn apply_normalized_event(
    event: Ev,
    filter: &Filter,
    machine: &mut Machine,
    repo_root: &Path,
) -> Result<(), String> {
    match event {
        Ev::Rescan => {
            machine.mark_scope_dirty(Instant::now());
            Ok(())
        }
        Ev::Error(msg) => Err(msg),
        Ev::Paths(paths) => {
            for path in paths {
                let rel = normalize_repo_relative(repo_root, &path);
                if filter.is_relevant(&rel) {
                    machine.note_path(rel, Instant::now(), Sig::from_path(&path));
                }
            }
            Ok(())
        }
    }
}

fn normalize_repo_relative(repo_root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(repo_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
#[path = "watch_test.rs"]
mod watch_test;
