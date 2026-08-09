//! Current-population unit-test wall timings from durable coverage caches.
//!
//! Gate policy and stats formatting live in higher layers; this module only
//! loads validated durable durations for the current Python/Rust populations.

use std::path::Path;
use std::time::{Duration, Instant};

use kiss::Language;
use kiss::stats::PercentileSummary;

use crate::test_runner::check_line_coverage::repository_root_for_universe;
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
    // Must match population publication / `load_python_runtime_coverage` pytest args.
    let pytest_args = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    let pairs = crate::test_runner::python_coverage_index::load_current_python_population_durations(
        repo_root,
        &pytest_args,
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
    ordered.sort_by(|a, b| {
        (a.language, a.selector.as_str()).cmp(&(b.language, b.selector.as_str()))
    });
    let mut lines = vec![format!(
        "GATE_FAILED:max_unit_test_seconds: {} test(s) exceeded path-pattern time limits",
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct CovTimeGateOpts<'a> {
    pub(crate) universe: &'a Path,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) include: TimingLangInclude,
    pub(crate) ignore: &'a [String],
    pub(crate) limits: &'a [(String, f64)],
    pub(crate) timing: bool,
}

/// Evaluate the unit-test time gate for `kiss cov`, with a fast path for sole `"*"`.
pub(crate) fn evaluate_cov_time_gate(opts: CovTimeGateOpts<'_>) -> RuntimeGateEval {
    if opts.limits.is_empty() {
        return RuntimeGateEval::Disabled;
    }
    if opts.limits.len() == 1 && opts.limits[0].0 == "*" {
        let fast = evaluate_sole_star_time_gate(opts, opts.limits[0].1);
        if !matches!(fast, RuntimeGateEval::Incomplete) {
            return fast;
        }
    }
    let t_timings = Instant::now();
    let timings = collect_current_unit_test_timings(TimingCollectOpts {
        universe: opts.universe,
        lang_filter: opts.lang_filter,
        include: opts.include,
        ignore: opts.ignore,
    });
    emit_timings_ms(opts.timing, t_timings);
    evaluate_runtime_gate(&timings, opts.limits)
}

fn evaluate_sole_star_time_gate(opts: CovTimeGateOpts<'_>, limit_seconds: f64) -> RuntimeGateEval {
    let t_timings = Instant::now();
    let repo_root = repository_root_for_universe(opts.universe);
    let want_python = opts.include.python
        && matches!(opts.lang_filter, None | Some(Language::Python));
    let want_rust =
        opts.include.rust && matches!(opts.lang_filter, None | Some(Language::Rust));
    let mut max = Duration::ZERO;
    if want_python {
        let pytest_args = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
        let Some(py_max) =
            crate::test_runner::python_coverage_index::load_current_python_population_max_duration(
                &repo_root,
                &pytest_args,
            )
        else {
            emit_timings_ms(opts.timing, t_timings);
            return RuntimeGateEval::Incomplete;
        };
        max = max.max(py_max);
    }
    if want_rust {
        match collect_current_unit_test_timings(TimingCollectOpts {
            universe: opts.universe,
            lang_filter: Some(Language::Rust),
            include: TimingLangInclude {
                python: false,
                rust: true,
            },
            ignore: opts.ignore,
        }) {
            TimingPopulation::Complete(rust) => {
                for t in &rust {
                    max = max.max(t.duration);
                }
            }
            TimingPopulation::Incomplete => {
                emit_timings_ms(opts.timing, t_timings);
                return RuntimeGateEval::Incomplete;
            }
        }
    }
    emit_timings_ms(opts.timing, t_timings);
    if max.as_secs_f64() < limit_seconds {
        return RuntimeGateEval::Passed;
    }
    let timings = collect_current_unit_test_timings(TimingCollectOpts {
        universe: opts.universe,
        lang_filter: opts.lang_filter,
        include: TimingLangInclude {
            python: want_python,
            rust: want_rust,
        },
        ignore: opts.ignore,
    });
    evaluate_runtime_gate(&timings, opts.limits)
}

fn emit_timings_ms(timing: bool, started: Instant) {
    if timing {
        eprintln!(
            "TIMING:coverage_unit_test_timings_ms:{}",
            started.elapsed().as_millis()
        );
    }
}

#[cfg(test)]
#[path = "unit_test_timing_test.rs"]
mod tests;
