use std::path::Path;
use std::time::Duration;

use kiss::Language;

use crate::test_runner::universe_root::repository_root_for_universe;

mod rust_durations;
use rust_durations::load_rust_duration_pairs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnitTestTiming {
    pub(crate) language: Language,
    pub(crate) selector: String,
    pub(crate) duration: Duration,
}

pub(crate) type TimingLangInclude = crate::test_runner::language_keyed::LanguageKeyed<bool>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TimingCollectOpts<'a> {
    pub(crate) universe: &'a Path,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) include: TimingLangInclude,
    pub(crate) ignore: &'a [String],
    pub(crate) pytest_args: &'a [String],
}

fn filter_timings_by_ignore(
    mut timings: Vec<UnitTestTiming>,
    ignore: &[String],
) -> Vec<UnitTestTiming> {
    if ignore.is_empty() {
        return timings;
    }
    timings.retain(|t| !selector_matches_ignore_prefix(&t.selector, ignore));
    timings
}

pub(super) fn selector_matches_ignore_prefix(selector: &str, ignore: &[String]) -> bool {
    kiss::selector_ignored_by_prefixes(selector, ignore)
}

fn load_python_timings(repo_root: &Path, pytest_args: &[String]) -> Option<Vec<UnitTestTiming>> {
    let pairs =
        crate::test_runner::python_coverage_index::load_current_python_population_durations(
            repo_root,
            pytest_args,
        )?;
    Some(
        pairs
            .into_iter()
            .map(|(selector, duration)| UnitTestTiming {
                language: Language::Python,
                selector,
                duration,
            })
            .collect(),
    )
}

fn load_rust_timings(repo_root: &Path) -> Option<Vec<UnitTestTiming>> {
    Some(map_rust_timing_pairs(
        repo_root,
        load_rust_duration_pairs(repo_root)?,
    ))
}

fn map_rust_timing_pairs(
    repo_root: &Path,
    pairs: Vec<(String, std::time::Duration)>,
) -> Vec<UnitTestTiming> {
    let selectors: Vec<String> = pairs.iter().map(|(selector, _)| selector.clone()).collect();
    let report_ids =
        crate::test_runner::runners::rust_report_ids_for_selectors(repo_root, &selectors)
            .unwrap_or_default();
    pairs
        .into_iter()
        .map(|(selector, duration)| UnitTestTiming {
            language: Language::Rust,
            selector: report_ids.get(&selector).cloned().unwrap_or(selector),
            duration,
        })
        .collect()
}

pub(crate) fn collect_available_unit_test_timings(
    opts: TimingCollectOpts<'_>,
) -> Vec<UnitTestTiming> {
    let want_python =
        opts.include.python && matches!(opts.lang_filter, None | Some(Language::Python));
    let want_rust = opts.include.rust && matches!(opts.lang_filter, None | Some(Language::Rust));
    let repo_root = repository_root_for_universe(opts.universe);
    let mut timings = Vec::new();
    if want_python && let Some(python) = load_python_timings(&repo_root, opts.pytest_args) {
        timings.extend(python);
    }
    if want_rust && let Some(rust) = load_rust_timings(&repo_root) {
        timings.extend(rust);
    }
    filter_timings_by_ignore(timings, opts.ignore)
}

fn cheap_codebase_test_count(
    universe: &Path,
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
    pytest_args: &[String],
) -> Option<usize> {
    let repo_root = repository_root_for_universe(universe);
    let need_python = include.python && matches!(lang_filter, None | Some(Language::Python));
    let need_rust = include.rust && matches!(lang_filter, None | Some(Language::Rust));
    let (py, rs) = super::workspace_selector_cache::load_workspace_selectors_for_count(
        &repo_root,
        ignore,
        pytest_args,
        super::workspace_selector_cache::SelectorCountNeed {
            python: need_python,
            rust: need_rust,
        },
    )?;
    Some(py.len() + rs.len())
}

pub(crate) fn unit_test_runtime_sec_report_for_universe(
    universe: &Path,
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
    rules: &[(String, f64)],
    pytest_args: &[String],
) -> Option<String> {
    let timings = collect_available_unit_test_timings(TimingCollectOpts {
        universe,
        lang_filter,
        include,
        ignore,
        pytest_args,
    });
    let codebase_tests =
        cheap_codebase_test_count(universe, lang_filter, include, ignore, pytest_args);
    let report = build_unit_test_runtime_grouped_report(&timings, rules, codebase_tests)?;
    Some(format_unit_test_runtime_grouped_report(&report))
}

mod runtime_report;
pub(crate) use runtime_report::{
    build_unit_test_runtime_grouped_report, format_unit_test_runtime_grouped_report,
};

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
