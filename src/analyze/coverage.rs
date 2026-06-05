use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use kiss::check_universe_cache::CachedCoverageItem;
use kiss::cli_output::file_coverage_map;
use kiss::graph::is_entry_point;
use kiss::{DependencyGraph, Violation};

pub(crate) use crate::analyze::coverage_types::{CoverageViolationSpec, PyRsTestCoverage};
use crate::analyze::coverage_weighted::{inject_binary_entry_sentinels, merge_weighted_file_pcts};
use crate::analyze::coverage_gate::{is_coverage_gate_file, is_coverage_report_target};
use crate::analyze::focus::is_focus_file;
use crate::analyze::graph_api::graph_for_path;

/// Graph pair for coverage / orphan resolution.
#[derive(Clone, Copy)]
pub(crate) struct GraphRefPair<'a> {
    pub py: Option<&'a DependencyGraph>,
    pub rs: Option<&'a DependencyGraph>,
}

/// Gate bypass and timing affect whether per-definition coverage violations are emitted.
pub(crate) struct CoverageOutputOpts {
    pub bypass_gate: bool,
    pub show_timing: bool,
}

/// Ensures definitions in orphan modules (`fan_in`==0, `fan_out`==0) are in unreferenced.
pub(crate) fn orphan_post_pass(
    definitions: &[CachedCoverageItem],
    unreferenced: Vec<CachedCoverageItem>,
    graphs: GraphRefPair<'_>,
) -> Vec<CachedCoverageItem> {
    let unref_set: HashSet<_> = unreferenced
        .iter()
        .map(|c| (c.file.clone(), c.name.clone(), c.line))
        .collect();
    let mut out = unreferenced;
    for def in definitions {
        let path = std::path::Path::new(&def.file);
        let Some(g) = graph_for_path(path, graphs.py, graphs.rs) else {
            continue;
        };
        let Some(module) = g.module_for_path(path) else {
            continue;
        };
        let metrics = g.module_metrics(&module);
        let is_orphan = metrics.fan_in == 0 && metrics.fan_out == 0 && !is_entry_point(&module);
        if is_orphan && !unref_set.contains(&(def.file.clone(), def.name.clone(), def.line)) {
            out.push(def.clone());
        }
    }
    out
}

pub(crate) fn build_coverage_violation_with_graph(
    spec: CoverageViolationSpec,
    graphs: GraphRefPair<'_>,
) -> Violation {
    let CoverageViolationSpec {
        file,
        name,
        line,
        file_pct,
    } = spec;
    let mut message = format!("{file_pct}% covered. Add test coverage for this code unit.");
    let mut suggestion = String::new();

    let graph = graph_for_path(&file, graphs.py, graphs.rs);

    if let Some(g) = graph
        && let Some(module) = g.module_for_path(&file)
    {
        let metrics = g.module_metrics(&module);
        if metrics.fan_in == 0 && !is_entry_point(&module) {
            message.push_str(" No test module imports this module.");
            suggestion = "Add an import in a test file, or remove if dead.".to_string();
        }
        let candidates = g.test_importers_of(&module);
        if !candidates.is_empty() {
            let truncated = kiss::cli_output::format_candidate_list(&candidates, 3);
            let _ = std::fmt::Write::write_fmt(
                &mut message,
                format_args!(" (candidates: {truncated})"),
            );
        }
    }

    Violation {
        file,
        line,
        unit_name: name,
        metric: "test_coverage".to_string(),
        value: 0,
        threshold: 0,
        message,
        suggestion,
    }
}

type CoverageCachePair = (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>);

pub(crate) fn merge_coverage_results(
    py_cov: kiss::TestRefAnalysis,
    rs_cov: kiss::RustTestRefAnalysis,
) -> (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>) {
    let mut definitions: Vec<CachedCoverageItem> = py_cov
        .definitions
        .into_iter()
        .map(|d| CachedCoverageItem {
            file: d.file.to_string_lossy().to_string(),
            name: d.name,
            line: d.line,
        })
        .collect();
    definitions.extend(rs_cov.definitions.into_iter().map(|d| CachedCoverageItem {
        file: d.file.to_string_lossy().to_string(),
        name: d.name,
        line: d.line,
    }));
    let mut unreferenced: Vec<CachedCoverageItem> = py_cov
        .unreferenced
        .into_iter()
        .map(|d| CachedCoverageItem {
            file: d.file.to_string_lossy().to_string(),
            name: d.name,
            line: d.line,
        })
        .collect();
    unreferenced.extend(rs_cov.unreferenced.into_iter().map(|d| CachedCoverageItem {
        file: d.file.to_string_lossy().to_string(),
        name: d.name,
        line: d.line,
    }));
    (definitions, unreferenced)
}

pub fn compute_test_coverage_from_lists(
    defs: &[(PathBuf, String, usize)],
    unref: &[(PathBuf, String, usize)],
    focus_set: &HashSet<PathBuf>,
) -> (usize, usize, usize, Vec<(PathBuf, String, usize)>) {
    let mut total = 0usize;
    let mut untested = 0usize;
    let mut unreferenced = Vec::new();

    for (file, _, _) in defs {
        if is_focus_file(file, focus_set) {
            total += 1;
        }
    }
    for (file, name, line) in unref {
        if is_focus_file(file, focus_set) {
            untested += 1;
            unreferenced.push((file.clone(), name.clone(), *line));
        }
    }
    unreferenced.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    let tested = total.saturating_sub(untested);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let coverage = if total > 0 {
        ((tested as f64 / total as f64) * 100.0).round() as usize
    } else {
        100
    };
    (coverage, tested, total, unreferenced)
}

pub(crate) fn build_viols_after_merge(
    definitions: Vec<CachedCoverageItem>,
    unreferenced: Vec<CachedCoverageItem>,
    focus_set: &HashSet<PathBuf>,
    graphs: GraphRefPair<'_>,
    weighted_pcts: Option<&HashMap<PathBuf, usize>>,
    report_entry_points: bool,
) -> (
    Vec<Violation>,
    Vec<CachedCoverageItem>,
    Vec<CachedCoverageItem>,
) {
    let unreferenced = orphan_post_pass(&definitions, unreferenced, graphs);
    let defs: Vec<_> = definitions
        .iter()
        .cloned()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let unref: Vec<_> = unreferenced
        .iter()
        .cloned()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let (_, _, _, mut unreferenced_focus) = compute_test_coverage_from_lists(&defs, &unref, focus_set);
    let mut file_pcts = file_coverage_map(&defs, &unreferenced_focus);
    if let Some(weighted) = weighted_pcts {
        for (path, pct) in weighted {
            file_pcts.insert(path.clone(), *pct);
            if *pct < 100
                && is_focus_file(path, focus_set)
                && is_coverage_gate_file(path, "")
                && !unreferenced_focus.iter().any(|(f, _, _)| f == path)
                && let Some(def) = defs.iter().find(|(f, _, _)| f == path)
            {
                unreferenced_focus.push(def.clone());
            }
        }
    }
    let cov_viols: Vec<Violation> = unreferenced_focus
        .into_iter()
        .filter(|(path, name, _)| is_coverage_report_target(path, name, report_entry_points))
        .map(|(file, name, line)| {
            let pct = file_pcts.get(&file).copied().unwrap_or(0);
            build_coverage_violation_with_graph(
                CoverageViolationSpec {
                    file,
                    name,
                    line,
                    file_pct: pct,
                },
                graphs,
            )
        })
        .collect();
    (cov_viols, definitions, unreferenced)
}

pub(crate) fn print_coverage_map_if_requested(weighted: &HashMap<PathBuf, usize>) {
    if std::env::var("KISS_COVERAGE_MAP").ok().as_deref() != Some("1") {
        return;
    }
    let mut entries: Vec<_> = weighted.iter().collect();
    entries.sort_by_key(|(path, _)| path.as_os_str());
    for (path, pct) in entries {
        println!("COVERAGE_MAP:{}:{pct}", path.display());
    }
}

pub(crate) fn collect_coverage_viols(
    cov: PyRsTestCoverage,
    py_parsed: &[kiss::ParsedFile],
    rs_parsed: &[kiss::ParsedRustFile],
    focus_set: &HashSet<PathBuf>,
    out_opts: CoverageOutputOpts,
    graphs: GraphRefPair<'_>,
    rs_files: &[PathBuf],
) -> (Vec<Violation>, Option<CoverageCachePair>) {
    let PyRsTestCoverage {
        py: py_cov,
        rs: rs_cov,
    } = cov;
    let weighted = merge_weighted_file_pcts(&py_cov, py_parsed, &rs_cov, rs_parsed);
    print_coverage_map_if_requested(&weighted);
    let (mut definitions, mut unreferenced) = merge_coverage_results(py_cov, rs_cov);
    if out_opts.bypass_gate {
        inject_binary_entry_sentinels(&mut definitions, &mut unreferenced, rs_files);
    }
    let (cov_viols, definitions, unreferenced) = build_viols_after_merge(
        definitions,
        unreferenced,
        focus_set,
        graphs,
        Some(&weighted),
        out_opts.bypass_gate,
    );
    let cov_viols = if out_opts.bypass_gate {
        cov_viols
    } else {
        Vec::new()
    };
    let cache_lists = if out_opts.show_timing {
        None
    } else {
        Some((definitions, unreferenced))
    };
    if out_opts.bypass_gate {
        (cov_viols, cache_lists)
    } else {
        (Vec::new(), cache_lists)
    }
}
