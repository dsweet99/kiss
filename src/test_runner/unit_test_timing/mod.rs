use std::path::Path;
use std::time::Duration;

use kiss::Language;

use crate::test_runner::check_line_coverage::repository_root_for_universe;
use crate::test_runner::rust_coverage_index::{
    resolved_rust_batch_request_parts, rust_coverage_cache_root,
};
use crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached;
use crate::test_runner::selector_ids::report_string_for_logical_string;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnitTestTiming {
    pub(crate) language: Language,
    pub(crate) selector: String,
    pub(crate) duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TimingPopulation {
    Complete(Vec<UnitTestTiming>),
    Incomplete,
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

pub(crate) fn collect_current_unit_test_timings(opts: TimingCollectOpts<'_>) -> TimingPopulation {
    let want_python =
        opts.include.python && matches!(opts.lang_filter, None | Some(Language::Python));
    let want_rust = opts.include.rust && matches!(opts.lang_filter, None | Some(Language::Rust));
    if !want_python && !want_rust {
        return TimingPopulation::Complete(Vec::new());
    }
    let repo_root = repository_root_for_universe(opts.universe);
    let mut timings = Vec::new();
    if want_python {
        match load_python_timings(&repo_root, opts.pytest_args) {
            Some(python) => timings.extend(python),
            None => return TimingPopulation::Incomplete,
        }
    }
    if want_rust {
        match load_rust_timings(&repo_root) {
            Some(rust) => timings.extend(rust),
            None => return TimingPopulation::Incomplete,
        }
    }
    TimingPopulation::Complete(filter_timings_by_ignore(timings, opts.ignore))
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
    let path_part = selector.split_once("::").map_or(selector, |(p, _)| p);
    kiss::path_ignored_by_prefixes(path_part, ignore)
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
    let cache_root = rust_coverage_cache_root(repo_root);
    let (req, tools) = resolved_rust_batch_request_parts(repo_root, &[]).ok()?;
    let identity = match kiss::rust_llvm_cov_runner::try_source_matched_seal_identity(
        &cache_root, repo_root,
    ) {
        Some(sealed)
            if kiss::rust_llvm_cov_runner::load_current_population_state(
                &cache_root,
                repo_root,
                &sealed,
                None,
            )
            .is_some() =>
        {
            sealed
        }
        _ => kiss::rust_llvm_cov_runner::batch_identity(&req, &tools).ok()?,
    };
    if let Some(pairs) = kiss::rust_llvm_cov_runner::load_current_population_durations(
        &cache_root,
        repo_root,
        &identity,
        &req,
        &tools,
        None,
    ) && !pairs.is_empty()
    {
        return Some(map_rust_timing_pairs(repo_root, pairs));
    }
    load_rust_timings_from_witness(repo_root, &identity)
}

fn map_rust_timing_pairs(
    repo_root: &Path,
    pairs: Vec<(String, std::time::Duration)>,
) -> Vec<UnitTestTiming> {
    let report_ids = rust_logical_to_kiss_test_ids_cached(repo_root, &[]).unwrap_or_default();
    pairs
        .into_iter()
        .map(|(selector, duration)| UnitTestTiming {
            language: Language::Rust,
            selector: report_string_for_logical_string(&report_ids, &selector),
            duration,
        })
        .collect()
}

fn load_rust_timings_from_witness(
    repo_root: &Path,
    identity: &kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity,
) -> Option<Vec<UnitTestTiming>> {
    use crate::test_runner::execution_witness::{
        rust_identity_digest_from_batch, try_load_rust_execution_witness,
    };
    use crate::test_runner::lang_iface::identity_covers;
    let witness = try_load_rust_execution_witness(repo_root).ok()?;
    if !identity_covers(
        &witness.identity_digest,
        &rust_identity_digest_from_batch(identity),
    ) {
        return None;
    }
    if !witness.complete
        || witness.selectors.is_empty()
        || witness.durations_ns.len() != witness.selectors.len()
    {
        return None;
    }
    let report_ids = rust_logical_to_kiss_test_ids_cached(repo_root, &[]).ok()?;
    witness
        .selectors
        .iter()
        .zip(witness.durations_ns.iter())
        .map(|(selector, &ns)| {
            let duration = Duration::from_nanos(ns?);
            Some(UnitTestTiming {
                language: Language::Rust,
                selector: report_string_for_logical_string(&report_ids, selector),
                duration,
            })
        })
        .collect::<Option<Vec<_>>>()
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuntimeGateViolation {
    pub(crate) language: Language,
    pub(crate) selector: String,
    pub(crate) seconds: f64,
    pub(crate) limit_seconds: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RuntimeGateEval {
    Disabled,
    Passed,
    Failed(Vec<RuntimeGateViolation>),
    Incomplete,
}

pub(crate) fn evaluate_runtime_gate(
    timings: &TimingPopulation,
    max_unit_test_seconds: &[(String, f64)],
) -> RuntimeGateEval {
    if max_unit_test_seconds.is_empty() {
        return RuntimeGateEval::Disabled;
    }
    match timings {
        TimingPopulation::Complete(entries) => {
            let viols: Vec<RuntimeGateViolation> = entries
                .iter()
                .filter_map(|t| {
                    let limit =
                        kiss::gate_config::limit_for_selector(max_unit_test_seconds, &t.selector);
                    let seconds = t.duration.as_secs_f64();
                    if seconds >= limit {
                        Some(RuntimeGateViolation {
                            language: t.language,
                            selector: t.selector.clone(),
                            seconds,
                            limit_seconds: limit,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            if viols.is_empty() {
                RuntimeGateEval::Passed
            } else {
                RuntimeGateEval::Failed(viols)
            }
        }
        TimingPopulation::Incomplete => RuntimeGateEval::Incomplete,
    }
}

pub(crate) fn runtime_gate_failure_lines(viols: &[RuntimeGateViolation]) -> Vec<String> {
    let mut ordered: Vec<&RuntimeGateViolation> = viols.iter().collect();
    ordered
        .sort_by(|a, b| (a.language, a.selector.as_str()).cmp(&(b.language, b.selector.as_str())));
    let mut lines = vec![format!(
        "VIOLATION:max_unit_test_seconds: {} test(s) exceeded path-pattern time limits",
        ordered.len()
    )];
    for v in ordered {
        lines.push(format!(
            "  [{}] {}: {:.2}s (limit {:.2}s)",
            v.language.label(),
            v.selector,
            v.seconds,
            v.limit_seconds
        ));
    }
    lines
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

pub(crate) fn codebase_test_count_for_cov(
    universe: &Path,
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
    pytest_args: &[String],
) -> Option<usize> {
    if let Some(n) = cheap_codebase_test_count(universe, lang_filter, include, ignore, pytest_args)
    {
        return Some(n);
    }
    match collect_current_unit_test_timings(TimingCollectOpts {
        universe,
        lang_filter,
        include,
        ignore,
        pytest_args,
    }) {
        TimingPopulation::Complete(entries) => Some(entries.len()),
        TimingPopulation::Incomplete => None,
    }
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

mod cov_gate;
#[cfg(test)]
use cov_gate::evaluate_path_max_runtime_violations;
pub(crate) use cov_gate::{CovTimeGateOpts, evaluate_cov_time_gate};

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
