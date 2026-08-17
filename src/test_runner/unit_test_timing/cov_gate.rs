//! `kiss cov` unit-test time-gate evaluation (sole-`*` and multi-prefix path-max).

use std::path::Path;
use std::time::{Duration, Instant};

use kiss::Language;

use super::{
    RuntimeGateEval, RuntimeGateViolation, TimingCollectOpts, TimingLangInclude,
    TimingPopulation, collect_current_unit_test_timings, evaluate_runtime_gate,
    selector_matches_ignore_prefix,
};
use crate::test_runner::check_line_coverage::repository_root_for_universe;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CovTimeGateOpts<'a> {
    pub(crate) universe: &'a Path,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) include: TimingLangInclude,
    pub(crate) ignore: &'a [String],
    pub(crate) limits: &'a [(String, f64)],
    pub(crate) timing: bool,
    pub(crate) pytest_args: &'a [String],
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
    if let Some(fast) = try_evaluate_multi_prefix_path_max_gate(opts) {
        return fast;
    }
    let t_timings = Instant::now();
    let timings = collect_current_unit_test_timings(TimingCollectOpts {
        universe: opts.universe,
        lang_filter: opts.lang_filter,
        include: opts.include,
        ignore: opts.ignore,
        pytest_args: opts.pytest_args,
    });
    emit_timings_ms(opts.timing, t_timings);
    evaluate_runtime_gate(&timings, opts.limits)
}

/// Multi-prefix warm path: Python path→max rows + optional Rust pair scan.
fn try_evaluate_multi_prefix_path_max_gate(opts: CovTimeGateOpts<'_>) -> Option<RuntimeGateEval> {
    let want_python = opts.include.python
        && matches!(opts.lang_filter, None | Some(Language::Python));
    let want_rust =
        opts.include.rust && matches!(opts.lang_filter, None | Some(Language::Rust));
    if !want_python {
        return None;
    }
    let t_timings = Instant::now();
    let repo_root = repository_root_for_universe(opts.universe);
    let pytest_args = opts.pytest_args;
    let t_py = Instant::now();
    let path_maxes =
        crate::test_runner::python_coverage_index::load_current_python_population_path_maxes(
            &repo_root,
            pytest_args,
        )?;
    if opts.timing {
        eprintln!(
            "TIMING:coverage_unit_test_timings_path:path_max:{}:py_ms:{}",
            path_maxes.len(),
            t_py.elapsed().as_millis()
        );
    }
    let mut viols =
        evaluate_path_max_runtime_violations(&path_maxes, opts.limits, opts.ignore);
    if want_rust {
        let t_rs = Instant::now();
        match collect_current_unit_test_timings(TimingCollectOpts {
            universe: opts.universe,
            lang_filter: Some(Language::Rust),
            include: TimingLangInclude {
                python: false,
                rust: true,
            },
            ignore: opts.ignore,
            pytest_args: opts.pytest_args,
        }) {
            TimingPopulation::Complete(rust) => {
                if opts.timing {
                    eprintln!(
                        "TIMING:coverage_unit_test_timings_rust_ms:{}:n:{}",
                        t_rs.elapsed().as_millis(),
                        rust.len()
                    );
                }
                if let RuntimeGateEval::Failed(mut rust_viols) =
                    evaluate_runtime_gate(&TimingPopulation::Complete(rust), opts.limits)
                {
                    viols.append(&mut rust_viols);
                }
            }
            TimingPopulation::Incomplete => {
                emit_timings_ms(opts.timing, t_timings);
                return Some(RuntimeGateEval::Incomplete);
            }
        }
    }
    emit_timings_ms(opts.timing, t_timings);
    Some(if viols.is_empty() {
        RuntimeGateEval::Passed
    } else {
        RuntimeGateEval::Failed(viols)
    })
}

pub(super) fn evaluate_path_max_runtime_violations(
    path_maxes: &[crate::test_runner::python_coverage_index::generation::PathMaxDuration],
    limits: &[(String, f64)],
    ignore: &[String],
) -> Vec<RuntimeGateViolation> {
    let mut viols = Vec::new();
    for row in path_maxes {
        if selector_matches_ignore_prefix(&row.example_selector, ignore) {
            continue;
        }
        let limit = kiss::gate_config::limit_for_selector(limits, &row.example_selector);
        let seconds = Duration::from_nanos(row.max_duration_ns).as_secs_f64();
        if seconds >= limit {
            viols.push(RuntimeGateViolation {
                language: Language::Python,
                selector: row.example_selector.clone(),
                seconds,
                limit_seconds: limit,
            });
        }
    }
    viols
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
        let Some(py_max) =
            crate::test_runner::python_coverage_index::load_current_python_population_max_duration(
                &repo_root,
                opts.pytest_args,
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
            pytest_args: opts.pytest_args,
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
        pytest_args: opts.pytest_args,
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
