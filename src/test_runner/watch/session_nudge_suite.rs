#![cfg(unix)]

use super::*;
use crate::test_runner::test_mode_fixtures::{git_in, python_dry_run_args};
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
    python_dry_run_args(vec!["a.py".into()])
}

pub(super) fn commit_a_py(tmp: &tempfile::TempDir) -> PathBuf {
    let file = tmp.path().join("a.py");
    std::fs::write(&file, "x=1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    file
}

pub(super) fn publish_workspace_rows(
    repo: &std::path::Path,
    rows: &[(
        &str,
        &str,
        crate::test_runner::target_request::EffectiveStatus,
    )],
    exit_code: i32,
) {
    publish_rows_for_request(
        repo,
        &crate::test_runner::target_request::workspace_request(None, &[]),
        rows,
        exit_code,
    );
}

pub(super) fn publish_rows_for_request(
    repo: &std::path::Path,
    request: &crate::test_runner::target_request::TargetRequest,
    rows: &[(
        &str,
        &str,
        crate::test_runner::target_request::EffectiveStatus,
    )],
    exit_code: i32,
) {
    use crate::test_runner::target_request::{
        EffectiveStatus, ReportScope, SelectorRow, TargetReport, publish_report, resolve_only,
        slice_for,
    };
    use crate::test_runner::workspace_selector_cache::{
        store_python_workspace_selectors, store_rust_workspace_selectors,
    };
    assert!(store_python_workspace_selectors(repo, &[], &[], &[]));
    assert!(store_rust_workspace_selectors(repo, &[], &[]));
    let stamp = match resolve_only(repo, request) {
        Ok(resolved) => slice_for(repo, request, &resolved),
        Err(_) => {
            let workspace = crate::test_runner::target_request::workspace_request(None, &[]);
            let resolved = resolve_only(repo, &workspace).expect("resolve workspace");
            slice_for(repo, &workspace, &resolved)
        }
    };
    let built_rows: Vec<SelectorRow> = rows
        .iter()
        .map(|(lang, sel, status)| SelectorRow {
            language: (*lang).into(),
            selector: (*sel).into(),
            raw: match status {
                EffectiveStatus::Pass => "passed",
                EffectiveStatus::Fail => "failed",
                EffectiveStatus::Timeout => "timed_out",
            }
            .into(),
            effective: *status,
            duration_ns: None,
            provenance: "witness".into(),
        })
        .collect();
    let selectors: Vec<String> = built_rows.iter().map(|row| row.selector.clone()).collect();
    let scope = ReportScope::from_membership(Vec::new(), selectors, stamp.complete);
    let mut built =
        TargetReport::assembled_in(repo, request, scope, built_rows, stamp, exit_code, false);
    built.stamp.complete = true;
    publish_report(repo, request, &built).expect("publish request report");
}

pub(super) fn publish_pass_count(repo: &std::path::Path, n: usize) {
    let owned: Vec<(String, String)> = (0..n).map(|i| ("python".into(), format!("t{i}"))).collect();
    let rows: Vec<(
        &str,
        &str,
        crate::test_runner::target_request::EffectiveStatus,
    )> = owned
        .iter()
        .map(|(lang, sel)| {
            (
                lang.as_str(),
                sel.as_str(),
                crate::test_runner::target_request::EffectiveStatus::Pass,
            )
        })
        .collect();
    publish_workspace_rows(repo, &rows, 0);
}

pub(super) fn timeout_steps(n: usize) -> VecDeque<Result<Vec<NormalizedWatchEvent>, RecvTimeout>> {
    let mut steps = VecDeque::new();
    for _ in 0..n {
        steps.push_back(Err(RecvTimeout::Timeout));
    }
    steps.push_back(Err(RecvTimeout::Disconnected("done".into())));
    steps
}

#[path = "session_nudge_cov_test.rs"]
mod cov_tests;
#[path = "session_nudge_default_test.rs"]
mod default_tests;
#[path = "session_nudge_identity_test.rs"]
mod identity_tests;
#[path = "session_nudge_scenario_test.rs"]
mod scenario_tests;
#[path = "session_nudge_suite_report_test.rs"]
mod suite_report_tests;
#[path = "session_nudge_test.rs"]
mod tests;
