use crate::bin_cli::config_session::{config_provenance, load_configs, load_gate_config};
use kiss::cli_output::file_coverage_map;
use kiss::parsing::{ParsedFile, parse_files};
use kiss::rust_parsing::{ParsedRustFile, parse_rust_files};
use kiss::{
    Config, DependencyGraph, DuplicationConfig, GateConfig, Language, MetricStats, analyze_graph,
    analyze_rust_test_refs, analyze_test_refs, build_dependency_graph, build_rust_dependency_graph,
    cluster_duplicates_from_chunks, compute_file_metrics, compute_rust_file_metrics,
    compute_summaries, extract_chunks_for_duplication, extract_rust_chunks_for_duplication,
    find_source_files_with_ignore, format_stats_table,
};
use std::path::{Path, PathBuf};

/// Per-language parsed-and-analyzed bundle. Built once per language so the
/// summary can derive metrics, coverage, dups, and orphan counts from a single
/// parse pass.
struct LangAnalysis {
    graph: DependencyGraph,
    file_count: usize,
    code_unit_count: usize,
    statement_count: usize,
    stats: MetricStats,
    duplicate_clusters: usize,
    orphan_violations: usize,
}

pub fn run_stats_summary(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    let (py_files, rs_files) = collect_files(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }

    let gate = load_gate_config(None, false);
    let (py_cfg, rs_cfg) = load_configs(None, false);

    let py = analyze_python(&py_files, &py_cfg, &gate);
    let rs = analyze_rust(&rs_files, &rs_cfg, &gate);

    print_summary(paths, py.as_ref(), rs.as_ref());
}

fn collect_files(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut py_files: Vec<PathBuf> = Vec::new();
    let mut rs_files: Vec<PathBuf> = Vec::new();
    for path in paths {
        let root = Path::new(path);
        for sf in find_source_files_with_ignore(root, ignore) {
            let want = lang_filter.is_none() || lang_filter == Some(sf.language);
            if !want {
                continue;
            }
            match sf.language {
                Language::Python => py_files.push(sf.path),
                Language::Rust => rs_files.push(sf.path),
            }
        }
    }
    (py_files, rs_files)
}

fn analyze_python(files: &[PathBuf], config: &Config, gate: &GateConfig) -> Option<LangAnalysis> {
    if files.is_empty() {
        return None;
    }
    let parsed: Vec<ParsedFile> = parse_files(files).map_or_else(
        |_| Vec::new(),
        |rs| rs.into_iter().filter_map(Result::ok).collect(),
    );
    if parsed.is_empty() {
        return None;
    }
    let parsed_refs: Vec<&ParsedFile> = parsed.iter().collect();
    let graph = build_dependency_graph(&parsed_refs);

    let mut stats = MetricStats::collect(&parsed_refs);
    stats.collect_graph_metrics(&graph);

    let cov = analyze_test_refs(&parsed_refs, Some(&graph));
    let defs: Vec<_> = cov
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let unref: Vec<_> = cov
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let pcts = file_coverage_map(&defs, &unref);
    // One coverage value per parsed file. Files with no extractable definitions
    // are conservatively treated as 100% covered (vacuously true), matching the
    // convention used by `compute_test_coverage_from_lists`.
    let covs: Vec<usize> = parsed
        .iter()
        .map(|p| pcts.get(&p.path).copied().unwrap_or(100))
        .collect();
    stats.extend_inv_test_coverage(covs);

    let (statement_count, code_unit_count) = file_totals_py(&parsed_refs);
    let duplicate_clusters = if gate.duplication_enabled {
        let chunks = extract_chunks_for_duplication(&parsed_refs);
        let dup_cfg = DuplicationConfig {
            min_similarity: gate.min_similarity,
            ..Default::default()
        };
        cluster_duplicates_from_chunks(&chunks, &dup_cfg).len()
    } else {
        0
    };
    let orphan_violations = count_orphans(&graph, config, gate.orphan_module_enabled);

    Some(LangAnalysis {
        file_count: parsed_refs.len(),
        graph,
        code_unit_count,
        statement_count,
        stats,
        duplicate_clusters,
        orphan_violations,
    })
}

fn analyze_rust(files: &[PathBuf], config: &Config, gate: &GateConfig) -> Option<LangAnalysis> {
    if files.is_empty() {
        return None;
    }
    let parsed: Vec<ParsedRustFile> = parse_rust_files(files)
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    if parsed.is_empty() {
        return None;
    }
    let parsed_refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let graph = build_rust_dependency_graph(&parsed_refs);

    let mut stats = MetricStats::collect_rust(&parsed_refs);
    stats.collect_graph_metrics(&graph);

    let cov = analyze_rust_test_refs(&parsed_refs, Some(&graph));
    let defs: Vec<_> = cov
        .definitions
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let unref: Vec<_> = cov
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let pcts = file_coverage_map(&defs, &unref);
    let covs: Vec<usize> = parsed
        .iter()
        .map(|p| pcts.get(&p.path).copied().unwrap_or(100))
        .collect();
    stats.extend_inv_test_coverage(covs);

    let (statement_count, code_unit_count) = file_totals_rs(&parsed_refs);
    let duplicate_clusters = if gate.duplication_enabled {
        let chunks = extract_rust_chunks_for_duplication(&parsed_refs);
        let dup_cfg = DuplicationConfig {
            min_similarity: gate.min_similarity,
            ..Default::default()
        };
        cluster_duplicates_from_chunks(&chunks, &dup_cfg).len()
    } else {
        0
    };
    let orphan_violations = count_orphans(&graph, config, gate.orphan_module_enabled);

    Some(LangAnalysis {
        file_count: parsed_refs.len(),
        graph,
        code_unit_count,
        statement_count,
        stats,
        duplicate_clusters,
        orphan_violations,
    })
}

fn file_totals_py(parsed: &[&ParsedFile]) -> (usize, usize) {
    let mut stmts = 0usize;
    let mut units = 0usize;
    for p in parsed {
        let fm = compute_file_metrics(p);
        stmts += fm.statements;
        units += kiss::count_code_units(p);
    }
    (stmts, units)
}

fn file_totals_rs(parsed: &[&ParsedRustFile]) -> (usize, usize) {
    let mut stmts = 0usize;
    let mut units = 0usize;
    for p in parsed {
        let fm = compute_rust_file_metrics(p);
        stmts += fm.statements;
        units += kiss::extract_rust_code_units(p).len();
    }
    (stmts, units)
}

fn count_orphans(graph: &DependencyGraph, config: &Config, enabled: bool) -> usize {
    if !enabled {
        return 0;
    }
    analyze_graph(graph, config, true)
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .count()
}

fn print_summary(paths: &[String], py: Option<&LangAnalysis>, rs: Option<&LangAnalysis>) {
    println!(
        "kiss stats - Summary Statistics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );

    let total_files = py.map_or(0, |p| p.file_count) + rs.map_or(0, |r| r.file_count);
    let total_units = py.map_or(0, |p| p.code_unit_count) + rs.map_or(0, |r| r.code_unit_count);
    let total_stmts = py.map_or(0, |p| p.statement_count) + rs.map_or(0, |r| r.statement_count);
    let (mut nodes, mut edges) = (0usize, 0usize);
    if let Some(p) = py {
        nodes += p.graph.graph.node_count();
        edges += p.graph.graph.edge_count();
    }
    if let Some(r) = rs {
        nodes += r.graph.graph.node_count();
        edges += r.graph.graph.edge_count();
    }
    println!(
        "Analyzed: {total_files} files, {total_units} code_units, {total_stmts} statements, \
         {nodes} graph_nodes, {edges} graph_edges"
    );

    let dup_total = py.map_or(0, |p| p.duplicate_clusters) + rs.map_or(0, |r| r.duplicate_clusters);
    let orphan_total =
        py.map_or(0, |p| p.orphan_violations) + rs.map_or(0, |r| r.orphan_violations);
    println!("Violations: {dup_total} duplicate, {orphan_total} orphan\n");

    if let Some(p) = py {
        println!(
            "=== Python ({} files) ===\n{}\n",
            p.file_count,
            format_stats_table(&compute_summaries(&p.stats))
        );
    }
    if let Some(r) = rs {
        println!(
            "=== Rust ({} files) ===\n{}",
            r.file_count,
            format_stats_table(&compute_summaries(&r.stats))
        );
    }
}
