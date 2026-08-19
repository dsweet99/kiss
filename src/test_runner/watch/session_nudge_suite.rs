#![cfg(unix)]


use super::*;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::test_mode_fixtures::git_in;
use crate::test_runner::watch::event_source::{
    NormalizedWatchEvent, RecvTimeout, WatchEventSource,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

pub(super) struct NudgeScript {
    pub steps: VecDeque<Result<Vec<NormalizedWatchEvent>, RecvTimeout>>,
}

impl WatchEventSource for NudgeScript {
    fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
        match self.steps.pop_front() {
            Some(Err(RecvTimeout::Timeout)) => {
                std::thread::sleep(timeout.min(Duration::from_millis(20)));
                Err(RecvTimeout::Timeout)
            }
            Some(other) => other,
            None => Err(RecvTimeout::Disconnected("nudge-script-done".into())),
        }
    }
}

pub(super) fn py_dry_args() -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        invocation: TestInvocation::Targets(vec!["a.py".into()]),
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
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

pub(super) fn commit_a_py(tmp: &tempfile::TempDir) -> PathBuf {
    let file = tmp.path().join("a.py");
    std::fs::write(&file, "x=1\n").unwrap();
    assert!(git_in(tmp.path()).args(["add", "a.py"]).status().unwrap().success());
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    file
}

pub(super) fn timeout_steps(n: usize) -> VecDeque<Result<Vec<NormalizedWatchEvent>, RecvTimeout>> {
    let mut steps = VecDeque::new();
    for _ in 0..n {
        steps.push_back(Err(RecvTimeout::Timeout));
    }
    steps.push_back(Err(RecvTimeout::Disconnected("done".into())));
    steps
}

#[path = "session_nudge_test.rs"]
mod tests;
#[path = "session_nudge_cov_test.rs"]
mod cov_tests;
