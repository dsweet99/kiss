use std::path::{Path, PathBuf};

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;

use super::KissTestReport;

#[path = "suite_report_digest.rs"]
mod digest;
#[path = "suite_report_store.rs"]
mod store;
#[path = "suite_report_recap.rs"]
mod recap;
#[path = "suite_report_apply.rs"]
mod apply;

use apply::{apply_scoped, apply_unscoped, replay_all, replay_lang, scoped_report_has_lang};
use digest::suite_source_digests;
use store::{
    identity_matches, read_store, write_store, DurableSuiteRecap, FilterIdentity, SCHEMA_VERSION,
};

pub(super) fn load_fresh_suite_report(
    args: &RunTestCmdArgs<'_>,
    repo_root: Option<&Path>,
) -> Option<KissTestReport> {
    if !reuse_eligible(args) {
        return None;
    }
    let (repo, key) = repo_and_key(args, repo_root)?;
    let stored = read_store(&repo)?;
    if !identity_matches(&stored, &key.identity()) {
        return None;
    }
    match key.lang.as_deref() {
        None => replay_all(&stored, &key.digests.all),
        Some(lang) => replay_lang(&stored, lang, key.lang_digest(lang)?),
    }
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
    if let Some(existing) = read_store(&repo)
        && existing.schema_version == SCHEMA_VERSION
        && !identity_matches(&existing, &key.identity())
    {
        return;
    }
    let mut stored = read_store(&repo)
        .filter(|item| identity_matches(item, &key.identity()))
        .unwrap_or_else(|| DurableSuiteRecap {
            schema_version: SCHEMA_VERSION.to_string(),
            ignore: key.ignore.clone(),
            extra: key.extra.clone(),
            python_extra: key.python_extra.clone(),
            ..DurableSuiteRecap::default()
        });
    stored.schema_version = SCHEMA_VERSION.to_string();
    stored.ignore = key.ignore.clone();
    stored.extra = key.extra.clone();
    stored.python_extra = key.python_extra.clone();
    let result = match key.lang.as_deref() {
        None => apply_unscoped(&mut stored, report, &key.digests),
        Some(lang) => apply_scoped(&mut stored, report, lang, &key.digests),
    };
    if let Err(err) = result {
        persist_error(err);
        return;
    }
    if let Err(err) = write_store(&repo, &stored) {
        persist_error(err);
    }
}

fn persist_error(err: impl std::fmt::Display) {
    eprintln!("error: kiss test: failed to persist suite report: {err}");
}

pub(crate) fn durable_lang_reply(
    repo: &Path,
    lang: kiss::Language,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
) -> Option<(i32, String)> {
    let stored = matching_store(repo, ignore, extra, python_extra)?;
    let recap = match lang {
        kiss::Language::Python => stored.recaps.python,
        kiss::Language::Rust => stored.recaps.rust,
    }?;
    Some((recap.exit_code, recap.output))
}

pub(crate) fn durable_all_reply(
    repo: &Path,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
) -> Option<(i32, String)> {
    let stored = matching_store(repo, ignore, extra, python_extra)?;
    let recap = stored.recaps.all?;
    let counts = stored.suite.anonymous_passed
        + stored.suite.anonymous_failed
        + stored.suite.anonymous_timed_out
        + stored.suite.lang_passed.iter().sum::<usize>()
        + stored.suite.lang_failed.iter().sum::<usize>()
        + stored.suite.lang_timed_out.iter().sum::<usize>()
        + stored.suite.named.len();
    if counts == 0 {
        return None;
    }
    Some((stored.suite.exit_code, recap.output))
}

fn matching_store(
    repo: &Path,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
) -> Option<DurableSuiteRecap> {
    let stored = read_store(repo)?;
    identity_matches(
        &stored,
        &FilterIdentity {
            ignore,
            extra,
            python_extra,
        },
    )
    .then_some(stored)
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
    matches!(args.invocation, TestInvocation::All)
        && !args.dry_run
        && !report.interrupted
        && !report.engine_aborted
        && !report_vacuous(report)
        && args
            .lang_filter
            .is_none_or(|lang| scoped_report_has_lang(report, lang.label()))
}

fn report_vacuous(report: &KissTestReport) -> bool {
    let totals = report
        .totals
        .as_ref()
        .map_or(0, |item| item.passed + item.failed + item.timed_out);
    totals == 0
        && report.named.is_empty()
        && report.lang_passed == [0, 0]
        && report.lang_failed == [0, 0]
        && report.lang_timed_out == [0, 0]
}

struct SourceKey {
    digests: digest::SuiteDigests,
    lang: Option<String>,
    ignore: Vec<String>,
    extra: Vec<String>,
    python_extra: Vec<String>,
}

impl SourceKey {
    fn identity(&self) -> FilterIdentity<'_> {
        FilterIdentity {
            ignore: &self.ignore,
            extra: &self.extra,
            python_extra: &self.python_extra,
        }
    }

    fn lang_digest(&self, lang: &str) -> Option<&str> {
        match lang {
            "python" => Some(self.digests.python.as_str()),
            "rust" => Some(self.digests.rust.as_str()),
            _ => None,
        }
    }
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
    let digests = suite_source_digests(&repo, args.ignore).ok()?;
    Some((
        repo,
        SourceKey {
            digests,
            lang: args.lang_filter.map(|lang| lang.label().to_string()),
            ignore: args.ignore.to_vec(),
            extra: args.extra.to_vec(),
            python_extra: args.python_extra.to_vec(),
        },
    ))
}

#[cfg(test)]
#[path = "suite_report_test.rs"]
mod tests;
