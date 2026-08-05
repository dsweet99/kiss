//! Current-population unit-test wall timings from durable coverage caches.
//!
//! Gate policy and stats formatting live in higher layers; this module only
//! loads validated durable durations for the current Python/Rust populations.

use std::path::Path;
use std::time::Duration;

use kiss::Language;
use kiss::stats::PercentileSummary;

use crate::test_runner::check_line_coverage::repository_root_for_universe;
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, stored_python_universe_population,
};
use crate::test_runner::runners::{detect_rslip_versions, rslip_request_from_parts};
use crate::test_runner::rust_coverage_index::{
    resolved_rust_batch_request_parts, rust_coverage_cache_root,
};

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct TimingLangInclude {
    pub(crate) python: bool,
    pub(crate) rust: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TimingCollectOpts<'a> {
    pub(crate) universe: &'a Path,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) include: TimingLangInclude,
    pub(crate) ignore: &'a [String],
}

pub(crate) fn collect_current_unit_test_timings(opts: TimingCollectOpts<'_>) -> TimingPopulation {
    let want_python = opts.include.python
        && matches!(opts.lang_filter, None | Some(Language::Python));
    let want_rust =
        opts.include.rust && matches!(opts.lang_filter, None | Some(Language::Rust));
    if !want_python && !want_rust {
        return TimingPopulation::Complete(Vec::new());
    }
    let repo_root = repository_root_for_universe(opts.universe);
    let mut timings = Vec::new();
    if want_python {
        match load_python_timings(&repo_root) {
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

fn selector_matches_ignore_prefix(selector: &str, ignore: &[String]) -> bool {
    let path_part = selector.split_once("::").map_or(selector, |(p, _)| p);
    ignore.iter().any(|prefix| {
        path_part == prefix.as_str() || path_part.starts_with(&format!("{prefix}/"))
    })
}

fn load_python_timings(repo_root: &Path) -> Option<Vec<UnitTestTiming>> {
    let population = stored_python_universe_population(repo_root, &[], PYTHON_COVERAGE_ENV_KEYS)?;
    let (python_version, pytest_version) = detect_rslip_versions(repo_root).ok()?;
    let reqs = population
        .selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                &[],
                &python_version,
                &pytest_version,
                false,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let outcomes = rslip::load_cached_outcomes_many(&reqs);
    if outcomes.len() != population.selectors.len() {
        return None;
    }
    let mut out = Vec::with_capacity(population.selectors.len());
    for (selector, outcome) in population.selectors.iter().zip(outcomes) {
        let outcome = outcome.ok()??;
        if outcome.nodeid != *selector {
            return None;
        }
        out.push(UnitTestTiming {
            language: Language::Python,
            selector: selector.clone(),
            duration: outcome.duration,
        });
    }
    Some(out)
}

fn load_rust_timings(repo_root: &Path) -> Option<Vec<UnitTestTiming>> {
    let (req, tools) = resolved_rust_batch_request_parts(repo_root, &[]).ok()?;
    let identity = rust_llvm_cov_runner::batch_identity(&req, &tools).ok()?;
    let cache_root = rust_coverage_cache_root(repo_root);
    // Prefer population selectors from the validated current state (covers both
    // SelectorEntries and CheckAggregate populations) without scanning orphans.
    let pairs = rust_llvm_cov_runner::load_current_population_durations(
        &cache_root,
        repo_root,
        &identity,
        &req,
        &tools,
        None,
    )?;
    Some(
        pairs
            .into_iter()
            .map(|(selector, duration)| UnitTestTiming {
                language: Language::Rust,
                selector,
                duration,
            })
            .collect(),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuntimeGateViolation {
    pub(crate) language: Language,
    pub(crate) selector: String,
    pub(crate) seconds: f64,
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
    max_unit_test_seconds: f64,
) -> RuntimeGateEval {
    if max_unit_test_seconds == 0.0 {
        return RuntimeGateEval::Disabled;
    }
    match timings {
        TimingPopulation::Complete(entries) => {
            let viols: Vec<RuntimeGateViolation> = entries
                .iter()
                .filter(|t| t.duration.as_secs_f64() >= max_unit_test_seconds)
                .map(|t| RuntimeGateViolation {
                    language: t.language,
                    selector: t.selector.clone(),
                    seconds: t.duration.as_secs_f64(),
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

pub(crate) fn runtime_gate_failure_lines(
    viols: &[RuntimeGateViolation],
    limit_seconds: f64,
) -> Vec<String> {
    let mut ordered: Vec<&RuntimeGateViolation> = viols.iter().collect();
    ordered.sort_by(|a, b| {
        (a.language, a.selector.as_str()).cmp(&(b.language, b.selector.as_str()))
    });
    let mut lines = vec![format!(
        "GATE_FAILED:max_unit_test_seconds: {} test(s) at or above {:.2}s",
        ordered.len(),
        limit_seconds
    )];
    for v in ordered {
        lines.push(format!(
            "  [{}] {}: {:.2}s (limit {:.2}s)",
            v.language.label(),
            v.selector,
            v.seconds,
            limit_seconds
        ));
    }
    lines
}

pub(crate) fn format_unit_test_runtime_ms_line(timings: &[UnitTestTiming]) -> Option<String> {
    if timings.is_empty() {
        return None;
    }
    let values: Vec<usize> = timings
        .iter()
        .map(|t| {
            #[allow(clippy::cast_possible_truncation)]
            {
                t.duration.as_millis() as usize
            }
        })
        .collect();
    let summary = PercentileSummary::from_values("unit_test_runtime_ms", &values);
    Some(format!(
        "unit_test_runtime_ms: N={} p50={} p90={} p95={} p99={} max={}",
        summary.count, summary.p50, summary.p90, summary.p95, summary.p99, summary.max
    ))
}

pub(crate) fn unit_test_runtime_ms_line_for_universe(
    universe: &Path,
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
) -> Option<String> {
    match collect_current_unit_test_timings(TimingCollectOpts {
        universe,
        lang_filter,
        include,
        ignore,
    }) {
        TimingPopulation::Complete(timings) => format_unit_test_runtime_ms_line(&timings),
        TimingPopulation::Incomplete => None,
    }
}

#[cfg(test)]
#[path = "unit_test_timing_test.rs"]
mod tests;
