//! Main analysis pipeline for layout structure reporting (unit tests; not a CLI subcommand).
//!
//! Coordinates cycle detection, layering, and output generation to
//! recommend codebase structure improvements.
//!
//! TODO: Add clustering for cohesion analysis (Leiden/Louvain algorithm)
//! once the core layering functionality is stable.

// `kiss layout` CLI was removed; this module remains for unit tests and programmatic reuse.

use kiss::{DependencyGraph, Language, LayerInfo};
use kiss::{analyze_cycles, compute_layers, format_markdown};
use kiss::{LayoutAnalysis, LayoutMetrics, WhatIfAnalysis};
use petgraph::visit::EdgeRef;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Options for the layout analysis.
pub struct LayoutOptions<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore_prefixes: &'a [String],
    pub project_name: Option<String>,
}

/// Run the layout analysis and write output to the specified path or stdout.
pub fn run_layout(opts: &LayoutOptions<'_>, out_path: Option<&Path>) -> std::io::Result<()> {
    let (py_files, rs_files) =
        kiss::discovery::gather_files_by_lang(opts.paths, opts.lang_filter, opts.ignore_prefixes);

    if py_files.is_empty() && rs_files.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No source files found.",
        ));
    }

    let analysis = analyze_layout(&py_files, &rs_files, opts)?;
    let output = format_markdown(&analysis);

    if let Some(path) = out_path {
        std::fs::write(path, &output)?;
        eprintln!("Layout analysis written to: {}", path.display());
    } else {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(output.as_bytes())?;
    }

    Ok(())
}

fn analyze_layout(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    opts: &LayoutOptions<'_>,
) -> std::io::Result<LayoutAnalysis> {
    let mut combined_graph = DependencyGraph::new();

    if !py_files.is_empty() {
        let py_graph = crate::analyze::build_py_graph_from_files(py_files)?;
        merge_graph(&mut combined_graph, &py_graph, "py");
    }

    if !rs_files.is_empty() {
        let rs_graph = crate::analyze::build_rs_graph_from_files(rs_files);
        merge_graph(&mut combined_graph, &rs_graph, "rs");
    }

    let cycle_analysis = analyze_cycles(&combined_graph);
    let layer_info = compute_layers(&combined_graph);

    let project_name = opts
        .project_name
        .clone()
        .unwrap_or_else(|| derive_project_name(opts.paths));

    let metrics = LayoutMetrics {
        cycle_count: cycle_analysis.cycle_count(),
        layering_violations: count_layering_violations(&combined_graph, &layer_info),
        cross_directory_deps: count_cross_directory_deps(&combined_graph),
    };

    let what_if = if cycle_analysis.cycle_count() > 0 {
        Some(compute_what_if(&combined_graph, &cycle_analysis))
    } else {
        None
    };

    Ok(LayoutAnalysis {
        project_name,
        metrics,
        cycle_analysis,
        layer_info,
        what_if,
    })
}

fn derive_project_name(paths: &[String]) -> String {
    project_name_from_paths(paths)
        .or_else(project_name_from_cwd)
        .unwrap_or_else(|| "project".to_string())
}

pub(crate) fn project_name_from_paths(paths: &[String]) -> Option<String> {
    let p = paths.first()?;
    let c = Path::new(p).canonicalize().ok()?;
    c.file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

pub(crate) fn project_name_from_cwd() -> Option<String> {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
}

fn count_cross_directory_deps(graph: &DependencyGraph) -> usize {
    let mut count = 0;
    for edge in graph.graph.edge_references() {
        let from_name = &graph.graph[edge.source()];
        let to_name = &graph.graph[edge.target()];

        let from_path = graph.paths.get(from_name);
        let to_path = graph.paths.get(to_name);

        if let (Some(from_p), Some(to_p)) = (from_path, to_path) {
            let from_dir = from_p.parent();
            let to_dir = to_p.parent();
            if from_dir != to_dir {
                count += 1;
            }
        }
    }
    count
}

fn count_layering_violations(graph: &DependencyGraph, layer_info: &LayerInfo) -> usize {
    graph
        .graph
        .edge_references()
        .filter(|edge| {
            let from = &graph.graph[edge.source()];
            let to = &graph.graph[edge.target()];
            match (layer_info.layer_for(from), layer_info.layer_for(to)) {
                (Some(from_layer), Some(to_layer)) => from_layer < to_layer,
                _ => false,
            }
        })
        .count()
}

fn merge_graph(target: &mut DependencyGraph, source: &DependencyGraph, prefix: &str) {
    for (name, path) in &source.paths {
        let prefixed_name = format!("{prefix}:{name}");
        target.paths.insert(prefixed_name.clone(), path.clone());
        target
            .path_to_module
            .insert(path.clone(), prefixed_name.clone());
        target.get_or_create_node(&prefixed_name);
    }

    for name in source.nodes.keys() {
        let prefixed_name = format!("{prefix}:{name}");
        target.get_or_create_node(&prefixed_name);
    }

    for edge in source.graph.edge_references() {
        let from = format!("{prefix}:{}", source.graph[edge.source()]);
        let to = format!("{prefix}:{}", source.graph[edge.target()]);
        target.add_dependency(&from, &to);
    }
}

fn compute_what_if(
    graph: &DependencyGraph,
    cycle_analysis: &kiss::LayoutCycleAnalysis,
) -> WhatIfAnalysis {
    let breaks_to_apply: std::collections::HashSet<_> = cycle_analysis
        .cycles
        .iter()
        .map(|c| c.suggested_break.clone())
        .collect();

    let simulated_graph = clone_graph_without_edges(graph, &breaks_to_apply);

    let simulated_cycles = analyze_cycles(&simulated_graph);
    let simulated_layers = compute_layers(&simulated_graph);

    let improvement = if simulated_cycles.is_acyclic() {
        format!(
            "Breaking {} cycle{} results in a clean {}-layer architecture.",
            cycle_analysis.cycle_count(),
            if cycle_analysis.cycle_count() == 1 {
                ""
            } else {
                "s"
            },
            simulated_layers.num_layers()
        )
    } else {
        format!(
            "Breaking suggested edges reduces cycles from {} to {}.",
            cycle_analysis.cycle_count(),
            simulated_cycles.cycle_count()
        )
    };

    WhatIfAnalysis {
        remaining_cycles: simulated_cycles.cycle_count(),
        layer_count: simulated_layers.num_layers(),
        improvement_summary: improvement,
    }
}

fn clone_graph_without_edges(
    graph: &DependencyGraph,
    edges_to_skip: &std::collections::HashSet<(String, String)>,
) -> DependencyGraph {
    let mut new_graph = DependencyGraph::new();

    for (name, path) in &graph.paths {
        new_graph.paths.insert(name.clone(), path.clone());
        new_graph.path_to_module.insert(path.clone(), name.clone());
        new_graph.get_or_create_node(name);
    }

    for name in graph.nodes.keys() {
        new_graph.get_or_create_node(name);
    }

    for edge in graph.graph.edge_references() {
        let from = &graph.graph[edge.source()];
        let to = &graph.graph[edge.target()];
        if !edges_to_skip.contains(&(from.clone(), to.clone())) {
            new_graph.add_dependency(from, to);
        }
    }

    new_graph
}

#[cfg(test)]
#[path = "layout_test.rs"]
mod tests;
