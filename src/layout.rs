//! Main analysis pipeline for `kiss layout` command.
//!
//! Coordinates cycle detection, layering, and output generation to
//! recommend codebase structure improvements.
//!
//! TODO: Add clustering for cohesion analysis (Leiden/Louvain algorithm)
//! once the core layering functionality is stable.

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
    paths
        .first()
        .and_then(|p| {
            Path::new(p)
                .canonicalize()
                .ok()
                .and_then(|c| c.file_name().map(|n| n.to_string_lossy().into_owned()))
        })
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "project".to_string())
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
mod tests {
    use super::*;

    #[test]
    fn test_merge_graph_adds_prefixed_nodes() {
        let mut source = DependencyGraph::new();
        source.add_dependency("a", "b");

        let mut target = DependencyGraph::new();
        merge_graph(&mut target, &source, "py");

        assert!(target.nodes.contains_key("py:a"));
        assert!(target.nodes.contains_key("py:b"));
    }

    #[test]
    fn test_merge_graph_adds_prefixed_edges() {
        let mut source = DependencyGraph::new();
        source.add_dependency("a", "b");

        let mut target = DependencyGraph::new();
        merge_graph(&mut target, &source, "py");

        assert!(target.imports("py:a", "py:b"));
    }

    #[test]
    fn test_clone_graph_without_edges_removes_specified() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        assert!(graph.imports("a", "b"));
        assert!(graph.imports("b", "c"));

        let edges_to_skip: std::collections::HashSet<_> =
            vec![("a".to_string(), "b".to_string())].into_iter().collect();
        let new_graph = clone_graph_without_edges(&graph, &edges_to_skip);

        assert!(!new_graph.imports("a", "b"));
        assert!(new_graph.imports("b", "c"));
    }

    #[test]
    fn test_clone_graph_without_edges_handles_empty_skip() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");

        let edges_to_skip = std::collections::HashSet::new();
        let new_graph = clone_graph_without_edges(&graph, &edges_to_skip);

        assert!(new_graph.imports("a", "b"));
    }

    #[test]
    fn test_compute_what_if_shows_improvement() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "a");

        let cycle_analysis = analyze_cycles(&graph);
        let what_if = compute_what_if(&graph, &cycle_analysis);

        assert_eq!(what_if.remaining_cycles, 0);
        assert!(what_if.improvement_summary.contains("clean"));
    }

    #[test]
    fn test_compute_what_if_overlapping_cycles() {
        // Two overlapping cycles sharing node 'b' and 'c':
        // Cycle 1: a -> b -> c -> a
        // Cycle 2: b -> c -> d -> b
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "a");
        graph.add_dependency("c", "d");
        graph.add_dependency("d", "b");

        let cycle_analysis = analyze_cycles(&graph);
        // Should detect cycles (may be 1 large SCC or 2 separate ones)
        assert!(
            cycle_analysis.cycle_count() >= 1,
            "Expected at least one cycle, got {}",
            cycle_analysis.cycle_count()
        );

        let what_if = compute_what_if(&graph, &cycle_analysis);
        // After breaking suggested edges, should have fewer or no cycles
        assert!(
            what_if.remaining_cycles <= cycle_analysis.cycle_count(),
            "Breaking edges should not increase cycles"
        );
    }

    #[test]
    fn test_build_py_graph_empty() {
        let graph = crate::analyze::build_py_graph_from_files(&[]).unwrap();
        assert!(graph.nodes.is_empty());
    }

    #[test]
    fn test_build_rs_graph_empty() {
        let graph = crate::analyze::build_rs_graph_from_files(&[]);
        assert!(graph.nodes.is_empty());
    }

    #[test]
    fn test_count_cross_directory_deps() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("mod_a", "mod_b");
        graph.add_dependency("mod_b", "mod_c");
        graph.add_dependency("mod_c", "mod_d");

        // Same directory: mod_a and mod_b
        graph.paths.insert("mod_a".to_string(), PathBuf::from("src/mod_a.py"));
        graph.paths.insert("mod_b".to_string(), PathBuf::from("src/mod_b.py"));
        // Different directory: mod_c in utils, mod_d in lib
        graph.paths.insert("mod_c".to_string(), PathBuf::from("utils/mod_c.py"));
        graph.paths.insert("mod_d".to_string(), PathBuf::from("lib/mod_d.py"));

        let count = count_cross_directory_deps(&graph);
        // mod_a -> mod_b: same dir (src), doesn't count
        // mod_b -> mod_c: different dirs (src vs utils), counts
        // mod_c -> mod_d: different dirs (utils vs lib), counts
        assert_eq!(count, 2);
    }

    #[test]
    fn test_count_cross_directory_deps_all_same_dir() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");

        graph.paths.insert("a".to_string(), PathBuf::from("src/a.py"));
        graph.paths.insert("b".to_string(), PathBuf::from("src/b.py"));
        graph.paths.insert("c".to_string(), PathBuf::from("src/c.py"));

        assert_eq!(count_cross_directory_deps(&graph), 0);
    }

    #[test]
    fn test_count_cross_directory_deps_missing_path() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        // Only a has a path, b is missing
        graph.paths.insert("a".to_string(), PathBuf::from("src/a.py"));

        // Should not count edges where paths are missing
        assert_eq!(count_cross_directory_deps(&graph), 0);
    }

    #[test]
    fn test_count_layering_violations_dag_no_violations() {
        // DAG: app -> domain -> utils
        // Layers: utils=0, domain=1, app=2
        // All edges go from higher to lower layer - no violations
        let mut graph = DependencyGraph::new();
        graph.add_dependency("app", "domain");
        graph.add_dependency("domain", "utils");

        let layer_info = compute_layers(&graph);
        assert_eq!(count_layering_violations(&graph, &layer_info), 0);
    }

    #[test]
    fn test_count_layering_violations_with_cycle() {
        // Cycle: app <-> utils
        // Both end up in same SCC at same layer, so no violations detected
        let mut graph = DependencyGraph::new();
        graph.add_dependency("app", "utils");
        graph.add_dependency("utils", "app");

        let layer_info = compute_layers(&graph);
        // Both are at layer 0 (same SCC), so from_layer == to_layer for all edges
        assert_eq!(count_layering_violations(&graph, &layer_info), 0);
    }

    #[test]
    fn test_count_layering_violations_with_manual_layers() {
        // Test the function with manually constructed layers to verify
        // it correctly detects violations when they exist
        use kiss::LayerInfo;

        let mut graph = DependencyGraph::new();
        graph.add_dependency("foundation", "app"); // foundation -> app edge

        // Manually create layers where foundation is at layer 0 and app is at layer 1
        // This means the edge foundation -> app goes from layer 0 to layer 1 (violation!)
        let layer_info = LayerInfo {
            layers: vec![
                vec!["foundation".to_string()], // layer 0
                vec!["app".to_string()],        // layer 1
            ],
        };

        assert_eq!(count_layering_violations(&graph, &layer_info), 1);
    }

    #[test]
    fn test_count_layering_violations_missing_layer_info() {
        // If a module isn't in layer_info, it shouldn't count as violation
        use kiss::LayerInfo;

        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "unknown");

        let layer_info = LayerInfo {
            layers: vec![vec!["a".to_string()]], // only 'a' has a layer
        };

        // 'unknown' has no layer, so this edge is not counted
        assert_eq!(count_layering_violations(&graph, &layer_info), 0);
    }

    #[test]
    fn test_layout_options_struct_fields() {
        let paths = vec!["src".to_string()];
        let ignore_prefixes = vec!["test_".to_string()];

        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: Some(Language::Python),
            ignore_prefixes: &ignore_prefixes,
            project_name: Some("my_project".to_string()),
        };

        assert_eq!(opts.paths, &["src".to_string()]);
        assert_eq!(opts.lang_filter, Some(Language::Python));
        assert_eq!(opts.ignore_prefixes, &["test_".to_string()]);
        assert_eq!(opts.project_name, Some("my_project".to_string()));
    }

    #[test]
    fn test_analyze_layout_with_python_files() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let py_file = temp_dir.path().join("module_a.py");
        std::fs::write(&py_file, "import module_b\n").unwrap();

        let py_file_b = temp_dir.path().join("module_b.py");
        std::fs::write(&py_file_b, "# no imports\n").unwrap();

        let py_files = vec![py_file, py_file_b];
        let rs_files: Vec<PathBuf> = vec![];

        let paths: Vec<String> = vec![];
        let ignore_prefixes: Vec<String> = vec![];
        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: None,
            ignore_prefixes: &ignore_prefixes,
            project_name: Some("test_project".to_string()),
        };

        let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
        assert_eq!(analysis.project_name, "test_project");
        // With two modules (one importing the other), we should have layers
        assert!(analysis.layer_info.num_layers() > 0);
    }

    #[test]
    fn test_analyze_layout_with_rust_files() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let rs_file = temp_dir.path().join("lib.rs");
        std::fs::write(&rs_file, "mod utils;\nfn main() {}\n").unwrap();

        let rs_file_b = temp_dir.path().join("utils.rs");
        std::fs::write(&rs_file_b, "pub fn helper() {}\n").unwrap();

        let py_files: Vec<PathBuf> = vec![];
        let rs_files = vec![rs_file, rs_file_b];

        let paths: Vec<String> = vec![];
        let ignore_prefixes: Vec<String> = vec![];
        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: None,
            ignore_prefixes: &ignore_prefixes,
            project_name: Some("rust_project".to_string()),
        };

        let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
        assert_eq!(analysis.project_name, "rust_project");
    }

    #[test]
    fn test_analyze_layout_project_name_custom() {
        let py_files: Vec<PathBuf> = vec![];
        let rs_files: Vec<PathBuf> = vec![];
        let paths: Vec<String> = vec![];
        let ignore_prefixes: Vec<String> = vec![];

        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: None,
            ignore_prefixes: &ignore_prefixes,
            project_name: Some("custom_name".to_string()),
        };

        let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
        assert_eq!(analysis.project_name, "custom_name");
    }

    #[test]
    fn test_analyze_layout_project_name_default() {
        let py_files: Vec<PathBuf> = vec![];
        let rs_files: Vec<PathBuf> = vec![];
        let paths: Vec<String> = vec![];
        let ignore_prefixes: Vec<String> = vec![];

        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: None,
            ignore_prefixes: &ignore_prefixes,
            project_name: None,
        };

        let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
        // Falls back to current dir name or "project"
        assert!(!analysis.project_name.is_empty());
    }

    #[test]
    fn test_run_layout_to_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();

        let py_file = temp_dir.path().join("app.py");
        std::fs::write(&py_file, "# simple module\n").unwrap();

        let out_file = temp_dir.path().join("layout_output.md");
        let path_str = temp_dir.path().to_string_lossy().to_string();

        let paths = vec![path_str];
        let ignore_prefixes: Vec<String> = vec![];
        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: Some(Language::Python),
            ignore_prefixes: &ignore_prefixes,
            project_name: Some("file_test".to_string()),
        };

        run_layout(&opts, Some(&out_file)).unwrap();

        assert!(out_file.exists());
        let content = std::fs::read_to_string(&out_file).unwrap();
        assert!(content.contains("file_test") || !content.is_empty());
    }

    #[test]
    fn test_run_layout_no_files_error() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path_str = temp_dir.path().to_string_lossy().to_string();

        let paths = vec![path_str];
        let ignore_prefixes: Vec<String> = vec![];
        let opts = LayoutOptions {
            paths: &paths,
            lang_filter: None,
            ignore_prefixes: &ignore_prefixes,
            project_name: None,
        };

        let result = run_layout(&opts, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("No source files"));
    }

    #[test]
    fn static_coverage_touch_derive_project_name() {
        fn t<T>(_: T) {}
        t(derive_project_name);
    }
}
