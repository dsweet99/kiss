use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::rust_coverage_index::{create_new_file, unique_suffix};
use crate::test_runner::RunTestCmdArgs;
use kiss::rust_llvm_cov_runner::{WatchSuiteReport, WatchSuiteTotals};

use super::KissTestReport;

#[path = "suite_report_digest.rs"]
mod digest;
use digest::suite_source_digest;

const SCHEMA_VERSION: &str = "kiss-suite-report-v4";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredTotals {
    passed: usize,
    failed: usize,
    timed_out: usize,
    total_label: String,
    max_pass_label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredSuiteReport {
    schema_version: String,
    source_digest: String,
    lang: Option<String>,
    ignore: Vec<String>,
    extra: Vec<String>,
    python_extra: Vec<String>,
    exit_code: i32,
    error: Option<String>,
    output: String,
    lines: Vec<String>,
    totals: Option<StoredTotals>,
}

pub(super) fn load_fresh_suite_report(
    args: &RunTestCmdArgs<'_>,
    repo_root: Option<&Path>,
) -> Option<KissTestReport> {
    if !reuse_eligible(args) {
        return None;
    }
    let (repo, key) = repo_and_key(args, repo_root)?;
    let stored = read_store(&repo)?;
    if !key_matches(&stored, &key) {
        return None;
    }
    Some(report_from_stored(stored))
}

pub(super) fn persist_suite_report(
    args: &RunTestCmdArgs<'_>,
    report: &KissTestReport,
    repo_root: Option<&Path>,
) {
    if !persist_eligible(args, report) {
        return;
    }
    let Some((repo, key)) = repo_and_key(args, repo_root) else {
        persist_error("cannot compute source key");
        return;
    };
    let output = compact_recap(report);
    if output.is_empty() {
        persist_error("empty recap");
        return;
    }
    let stored = StoredSuiteReport {
        schema_version: SCHEMA_VERSION.to_string(),
        source_digest: key.source_digest,
        lang: key.lang,
        ignore: key.ignore,
        extra: key.extra,
        python_extra: key.python_extra,
        exit_code: report.exit_code,
        error: report.error.clone(),
        output,
        lines: report.lines.clone(),
        totals: report.totals.as_ref().map(stored_totals),
    };
    if let Err(err) = write_store(&repo, &stored) {
        persist_error(err);
    }
}

fn persist_error(err: impl std::fmt::Display) {
    eprintln!("error: kiss test: failed to persist suite report: {err}");
}

pub(super) fn replay_suite_report(report: &KissTestReport) {
    let Some(output) = report.output.as_deref() else {
        return;
    };
    if output.is_empty() {
        return;
    }
    print!("{output}");
    if !output.ends_with('\n') {
        println!();
    }
}

fn reuse_eligible(args: &RunTestCmdArgs<'_>) -> bool {
    matches!(args.invocation, TestInvocation::All)
        && !args.dry_run
        && !args.force_rerun
        && !args.force_bad
        && !args.metrics
}

fn persist_eligible(args: &RunTestCmdArgs<'_>, report: &KissTestReport) -> bool {
    matches!(args.invocation, TestInvocation::All) && !args.dry_run && !report.interrupted
}

struct SourceKey {
    source_digest: String,
    lang: Option<String>,
    ignore: Vec<String>,
    extra: Vec<String>,
    python_extra: Vec<String>,
}

fn repo_and_key(
    args: &RunTestCmdArgs<'_>,
    repo_root: Option<&Path>,
) -> Option<(PathBuf, SourceKey)> {
    let repo = match repo_root {
        Some(path) => path.to_path_buf(),
        None => {
            let cwd = std::env::current_dir().ok()?;
            crate::test_git::require_git_repo_root(&cwd).ok()?
        }
    };
    let source_digest = suite_source_digest(&repo, args.ignore).ok()?;
    Some((
        repo,
        SourceKey {
            source_digest,
            lang: args.lang_filter.map(|lang| lang.label().to_string()),
            ignore: args.ignore.to_vec(),
            extra: args.extra.to_vec(),
            python_extra: args.python_extra.to_vec(),
        },
    ))
}

fn key_matches(stored: &StoredSuiteReport, key: &SourceKey) -> bool {
    stored.schema_version == SCHEMA_VERSION
        && stored.source_digest == key.source_digest
        && stored.lang == key.lang
        && stored.ignore == key.ignore
        && stored.extra == key.extra
        && stored.python_extra == key.python_extra
}

fn compact_recap(report: &KissTestReport) -> String {
    let mut suite = WatchSuiteReport::default();
    suite.merge_unscoped_lines(&report.lines);
    if let Some(totals) = &report.totals {
        suite.apply_totals(totals);
    }
    if report.exit_code == 0 {
        suite.merge_lines(&["NO VIOLATIONS".into()]);
    }
    suite.format()
}

fn stored_totals(totals: &WatchSuiteTotals) -> StoredTotals {
    StoredTotals {
        passed: totals.passed,
        failed: totals.failed,
        timed_out: totals.timed_out,
        total_label: totals.total_label.clone(),
        max_pass_label: totals.max_pass_label.clone(),
    }
}

fn report_from_stored(stored: StoredSuiteReport) -> KissTestReport {
    KissTestReport {
        exit_code: stored.exit_code,
        output: Some(stored.output),
        lines: stored.lines,
        totals: stored.totals.map(|totals| WatchSuiteTotals {
            passed: totals.passed,
            failed: totals.failed,
            timed_out: totals.timed_out,
            total_label: totals.total_label,
            max_pass_label: totals.max_pass_label,
        }),
        error: stored.error,
        interrupted: false,
    }
}

fn suite_report_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("suite_report.json")
}

fn read_store(repo_root: &Path) -> Option<StoredSuiteReport> {
    let bytes = fs::read(suite_report_path(repo_root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_store(repo_root: &Path, stored: &StoredSuiteReport) -> Result<(), String> {
    let path = suite_report_path(repo_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss test: suite report path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".suite_report.{}.tmp", unique_suffix()));
    let mut file = create_new_file(&tmp_path).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, stored).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(tmp_path, path).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "suite_report_test.rs"]
mod tests;
