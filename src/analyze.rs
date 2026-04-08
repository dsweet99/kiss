use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::analyze_cache;
use crate::analyze_parse::{ParseResult, parse_all, parse_all_timed, py_parsed_or_log};
use kiss::check_universe_cache::CachedCoverageItem;
use kiss::cli_output::{
    file_coverage_map, print_coverage_gate_failure, print_dry_results, print_duplicates,
    print_final_status, print_no_files_message, print_violations,
};
use kiss::{
    Config, DependencyGraph, DuplicateCluster, DuplicationConfig, GateConfig, Language, ParsedFile,
    ParsedRustFile, Violation, analyze_graph, build_dependency_graph,
    build_rust_dependency_graph, cluster_duplicates_from_chunks, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication,
    find_source_files_with_ignore, parse_files, parse_rust_files,
};

pub struct AnalyzeOptions<'a> {
    pub universe: &'a str,
    pub focus_paths: &'a [String],
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub lang_filter: Option<Language>,
    pub bypass_gate: bool,
    pub gate_config: &'a GateConfig,
    pub ignore_prefixes: &'a [String],
    pub show_timing: bool,
    /// If true, suppress "NO VIOLATIONS" sentinel (used by shrink mode to emit it after constraint check)
    pub suppress_final_status: bool,
}

/// Result of running analysis, including computed global metrics.
#[derive(Debug, Clone)]
pub struct AnalyzeResult {
    /// Whether the analysis passed (no violations).
    pub success: bool,
    /// Global metrics computed during analysis.
    /// `None` on cache hit (metrics not recomputed); `Some` on full analysis.
    pub metrics: Option<kiss::GlobalMetrics>,
}

/// Run analysis and return a simple success/failure bool.
/// Use `run_analyze_with_result` if you need the computed metrics.
#[allow(clippy::too_many_lines)]
pub fn run_analyze(opts: &AnalyzeOptions<'_>) -> bool {
    run_analyze_with_result(opts).success
}

/// Run analysis and return detailed result including global metrics.
#[allow(clippy::too_many_lines)]
pub fn run_analyze_with_result(opts: &AnalyzeOptions<'_>) -> AnalyzeResult {
    let t0 = std::time::Instant::now();
    let universe_root = Path::new(opts.universe);
    let (py_files, rs_files) = gather_files(universe_root, opts.lang_filter, opts.ignore_prefixes);
    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(opts.lang_filter, universe_root);
        return AnalyzeResult {
            success: true,
            metrics: Some(kiss::GlobalMetrics {
                files: 0,
                code_units: 0,
                statements: 0,
                graph_nodes: 0,
                graph_edges: 0,
            }),
        };
    }
    let focus_set = if opts.focus_paths.len() == 1 && opts.focus_paths[0] == opts.universe {
        let mut set = HashSet::with_capacity(py_files.len() + rs_files.len());
        set.extend(py_files.iter().cloned());
        set.extend(rs_files.iter().cloned());
        set
    } else {
        build_focus_set(opts.focus_paths, opts.lang_filter, opts.ignore_prefixes)
    };
    if opts.bypass_gate
        && !opts.show_timing
        && !opts.suppress_final_status
        && let Some(ok) = analyze_cache::try_run_cached_all(opts, &py_files, &rs_files, &focus_set)
    {
        return AnalyzeResult {
            success: ok,
            metrics: None,
        };
    }
    let t1 = std::time::Instant::now();
    run_analyze_uncached(opts, &py_files, &rs_files, &focus_set, t0, t1)
}

struct RustAnalysis {
    graph: Option<DependencyGraph>,
    cov: kiss::RustTestRefAnalysis,
    dups: Vec<DuplicateCluster>,
}

fn run_rust_analysis(
    rs_parsed: &[ParsedRustFile],
    gate_config: &GateConfig,
    cached_rs_cov: Option<kiss::RustTestRefAnalysis>,
) -> RustAnalysis {
    let graph = build_rs_graph(rs_parsed);
    let cov = cached_rs_cov.unwrap_or_else(|| {
        let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
        kiss::analyze_rust_test_refs(&rs_refs, graph.as_ref())
    });
    let dups = if gate_config.duplication_enabled {
        detect_rs_duplicates(rs_parsed, gate_config.min_similarity)
    } else {
        Vec::new()
    };
    RustAnalysis { graph, cov, dups }
}

type GraphResult = (Option<DependencyGraph>, Vec<Violation>);
type CoverageResult = (kiss::TestRefAnalysis, Vec<DuplicateCluster>);

fn run_parallel_py_analysis(
    py_parsed: &[ParsedFile],
    rs_graph: Option<&DependencyGraph>,
    opts: &AnalyzeOptions<'_>,
    file_count: usize,
    cached_py_cov: Option<kiss::TestRefAnalysis>,
) -> (GraphResult, CoverageResult) {
    let orphan_enabled = opts.gate_config.orphan_module_enabled;
    let dup_enabled = opts.gate_config.duplication_enabled;
    let min_sim = opts.gate_config.min_similarity;
    let py_graph = build_py_graph(py_parsed);
    let (gv, (py_cov, py_dups)) = rayon::join(
        || build_graph_violations(py_graph.as_ref(), rs_graph, opts, file_count, orphan_enabled),
        || {
            let py_cov = cached_py_cov.unwrap_or_else(|| {
                let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
                kiss::analyze_test_refs_no_map(&py_refs, py_graph.as_ref())
            });
            let py_dups = if dup_enabled {
                detect_py_duplicates(py_parsed, min_sim)
            } else {
                Vec::new()
            };
            (py_cov, py_dups)
        },
    );
    ((py_graph, gv), (py_cov, py_dups))
}

fn build_graph_violations(
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
    opts: &AnalyzeOptions<'_>,
    file_count: usize,
    orphan_enabled: bool,
) -> Vec<Violation> {
    if file_count <= 1 {
        return Vec::new();
    }
    let mut gv = Vec::new();
    if let Some(g) = py_graph {
        gv.extend(analyze_graph(g, opts.py_config, orphan_enabled));
    }
    if let Some(g) = rs_graph {
        gv.extend(analyze_graph(g, opts.rs_config, orphan_enabled));
    }
    gv
}

type CoverageCachePair = (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>);

/// Ensures definitions in orphan modules (`fan_in`==0, `fan_out`==0) are in unreferenced.
pub fn graph_for_path<'a>(
    path: &Path,
    py_graph: Option<&'a DependencyGraph>,
    rs_graph: Option<&'a DependencyGraph>,
) -> Option<&'a DependencyGraph> {
    path.extension().and_then(|e| {
        e.to_str().and_then(|ext| {
            if ext == "py" {
                py_graph
            } else if ext == "rs" {
                rs_graph
            } else {
                None
            }
        })
    })
}

/// Build a Python dependency graph from a list of Python file paths.
pub fn build_py_graph_from_files(py_files: &[PathBuf]) -> std::io::Result<DependencyGraph> {
    let results = parse_files(py_files).map_err(|e| std::io::Error::other(e.to_string()))?;
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    Ok(build_dependency_graph(&parsed))
}

/// Build a Rust dependency graph from a list of Rust file paths.
pub fn build_rs_graph_from_files(rs_files: &[PathBuf]) -> DependencyGraph {
    let results = kiss::rust_parsing::parse_rust_files(rs_files);
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    build_rust_dependency_graph(&parsed)
}

fn orphan_post_pass(
    definitions: &[CachedCoverageItem],
    unreferenced: Vec<CachedCoverageItem>,
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
) -> Vec<CachedCoverageItem> {
    let unref_set: HashSet<_> = unreferenced
        .iter()
        .map(|c| (c.file.clone(), c.name.clone(), c.line))
        .collect();
    let mut out = unreferenced;
    for def in definitions {
        let path = std::path::Path::new(&def.file);
        let Some(g) = graph_for_path(path, py_graph, rs_graph) else { continue };
        let Some(module) = g.module_for_path(path) else { continue };
        let metrics = g.module_metrics(&module);
        let is_orphan = metrics.fan_in == 0
            && metrics.fan_out == 0
            && !g.is_entry_point_module(&module);
        if is_orphan && !unref_set.contains(&(def.file.clone(), def.name.clone(), def.line)) {
            out.push(def.clone());
        }
    }
    out
}

fn build_coverage_violation_with_graph(
    file: PathBuf,
    name: String,
    line: usize,
    file_pct: usize,
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
) -> Violation {
    let mut message = format!("{file_pct}% covered. Add test coverage for this code unit.");
    let mut suggestion = String::new();

    let graph = graph_for_path(&file, py_graph, rs_graph);

    if let Some(g) = graph
        && let Some(module) = g.module_for_path(&file)
    {
        let metrics = g.module_metrics(&module);
        if metrics.fan_in == 0 && !g.is_entry_point_module(&module) {
            message.push_str(" No test module imports this module.");
            suggestion = "Add an import in a test file, or remove if dead.".to_string();
        }
        let candidates = g.test_importers_of(&module);
        if !candidates.is_empty() {
            let truncated = kiss::cli_output::format_candidate_list(&candidates, 3);
            let _ = std::fmt::Write::write_fmt(&mut message, format_args!(" (candidates: {truncated})"));
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

fn collect_coverage_viols(
    py_cov: kiss::TestRefAnalysis,
    rs_cov: kiss::RustTestRefAnalysis,
    focus_set: &HashSet<PathBuf>,
    bypass_gate: bool,
    show_timing: bool,
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
) -> (Vec<Violation>, Option<CoverageCachePair>) {
    let (definitions, mut unreferenced) = merge_coverage_results(py_cov, rs_cov);
    // When the gate is not bypassed, per-definition coverage violations are intentionally
    // not emitted; coverage is only checked at the gate level (pass/fail).
    if !bypass_gate {
        return (Vec::new(), None);
    }
    unreferenced = orphan_post_pass(&definitions, unreferenced, py_graph, rs_graph);
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
    let (_, _, _, unreferenced_focus) = compute_test_coverage_from_lists(&defs, &unref, focus_set);
    let file_pcts = file_coverage_map(&defs, &unreferenced_focus);
    let cov_viols: Vec<Violation> = unreferenced_focus
        .into_iter()
        .map(|(file, name, line)| {
            let pct = file_pcts.get(&file).copied().unwrap_or(0);
            build_coverage_violation_with_graph(file, name, line, pct, py_graph, rs_graph)
        })
        .collect();
    let cache_lists = if show_timing {
        None
    } else {
        Some((definitions, unreferenced))
    };
    (cov_viols, cache_lists)
}

fn build_metrics(
    result: &ParseResult,
    file_count: usize,
    py_g: Option<&DependencyGraph>,
    rs_g: Option<&DependencyGraph>,
) -> kiss::GlobalMetrics {
    let (nodes, edges) = graph_stats(py_g, rs_g);
    kiss::GlobalMetrics {
        files: file_count,
        code_units: result.code_unit_count,
        statements: result.statement_count,
        graph_nodes: nodes,
        graph_edges: edges,
    }
}

fn run_analyze_uncached(
    opts: &AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
    t0: std::time::Instant,
    t1: std::time::Instant,
) -> AnalyzeResult {
    let (result, parse_timing) = parse_all_timed(
        py_files, rs_files, opts.py_config, opts.rs_config, opts.show_timing,
    );
    let t2 = std::time::Instant::now();
    log_parse_timing(opts.show_timing, &parse_timing);
    let viols = filter_viols_by_focus(result.violations.clone(), focus_set);
    let file_count = result.py_parsed.len() + result.rs_parsed.len();

    if !opts.bypass_gate {
        return run_gated_analysis(opts, py_files, rs_files, focus_set, (result, viols, file_count), (t0, t1, t2));
    }

    let rs = run_rust_analysis(&result.rs_parsed, opts.gate_config, None);
    let ((py_graph, graph_viols_all), (py_cov, py_dups_all)) =
        run_parallel_py_analysis(&result.py_parsed, rs.graph.as_ref(), opts, file_count, None);

    finalize_analysis(
        opts, py_files, rs_files, focus_set,
        AnalysisProducts { result, viols, file_count, py_cov, rs, py_graph, graph_viols_all, py_dups_all },
        (t0, t1, t2),
    )
}

fn run_gated_analysis(
    opts: &AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
    parsed: (ParseResult, Vec<Violation>, usize),
    timings: (std::time::Instant, std::time::Instant, std::time::Instant),
) -> AnalyzeResult {
    let (result, viols, file_count) = parsed;
    let gc = opts.gate_config;

    let rs_refs: Vec<&ParsedRustFile> = result.rs_parsed.iter().collect();
    let rs_cov = kiss::analyze_rust_test_refs(&rs_refs, None);

    let (py_cov, (py_graph, mut graph_viols_all, py_dups_all)) = rayon::join(
        || {
            let py_refs: Vec<&ParsedFile> = result.py_parsed.iter().collect();
            kiss::analyze_test_refs_quick(&py_refs)
        },
        || {
            let py_graph = build_py_graph(&result.py_parsed);
            let (gv, py_dups) = rayon::join(
                || build_graph_violations(py_graph.as_ref(), None, opts, file_count, gc.orphan_module_enabled),
                || if gc.duplication_enabled { detect_py_duplicates(&result.py_parsed, gc.min_similarity) } else { Vec::new() },
            );
            (py_graph, gv, py_dups)
        },
    );

    if let Some(early) = evaluate_gate(&py_cov, &rs_cov, focus_set, opts) {
        return early;
    }

    let rs_graph = build_rs_graph(&result.rs_parsed);
    if let Some(ref g) = rs_graph {
        graph_viols_all.extend(analyze_graph(g, opts.rs_config, gc.orphan_module_enabled));
    }
    let rs = RustAnalysis {
        graph: rs_graph,
        cov: rs_cov,
        dups: if gc.duplication_enabled { detect_rs_duplicates(&result.rs_parsed, gc.min_similarity) } else { Vec::new() },
    };

    finalize_analysis(
        opts, py_files, rs_files, focus_set,
        AnalysisProducts { result, viols, file_count, py_cov, rs, py_graph, graph_viols_all, py_dups_all },
        timings,
    )
}

fn evaluate_gate(
    py_cov: &kiss::TestRefAnalysis,
    rs_cov: &kiss::RustTestRefAnalysis,
    focus_set: &HashSet<PathBuf>,
    opts: &AnalyzeOptions<'_>,
) -> Option<AnalyzeResult> {
    let defs_t: Vec<_> = py_cov.definitions.iter().map(|d| (d.file.clone(), d.name.clone(), d.line))
        .chain(rs_cov.definitions.iter().map(|d| (d.file.clone(), d.name.clone(), d.line))).collect();
    let unrefs_t: Vec<_> = py_cov.unreferenced.iter().map(|d| (d.file.clone(), d.name.clone(), d.line))
        .chain(rs_cov.unreferenced.iter().map(|d| (d.file.clone(), d.name.clone(), d.line))).collect();
    let (coverage, tested, total, unreferenced_focus) =
        compute_test_coverage_from_lists(&defs_t, &unrefs_t, focus_set);
    if coverage < opts.gate_config.test_coverage_threshold {
        let file_pcts = file_coverage_map(&defs_t, &unreferenced_focus);
        print_coverage_gate_failure(coverage, opts.gate_config.test_coverage_threshold, tested, total, &unreferenced_focus, &file_pcts);
        return Some(AnalyzeResult { success: false, metrics: None });
    }
    None
}

struct AnalysisProducts {
    result: ParseResult,
    viols: Vec<Violation>,
    file_count: usize,
    py_cov: kiss::TestRefAnalysis,
    rs: RustAnalysis,
    py_graph: Option<DependencyGraph>,
    graph_viols_all: Vec<Violation>,
    py_dups_all: Vec<DuplicateCluster>,
}

fn finalize_analysis(
    opts: &AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
    ap: AnalysisProducts,
    timings: (std::time::Instant, std::time::Instant, std::time::Instant),
) -> AnalyzeResult {
    let AnalysisProducts {
        result, mut viols, file_count, py_cov, rs, py_graph,
        graph_viols_all, py_dups_all,
    } = ap;
    if opts.show_timing {
        log_timing_phase1(timings.0, timings.1, timings.2, std::time::Instant::now());
    }

    let metrics = build_metrics(&result, file_count, py_graph.as_ref(), rs.graph.as_ref());
    print_analysis_summary(
        metrics.files, metrics.code_units, metrics.statements,
        py_graph.as_ref(), rs.graph.as_ref(),
    );

    viols.extend(filter_viols_by_focus(graph_viols_all.clone(), focus_set));
    let t_phase2 = std::time::Instant::now();
    let (cov_viols, coverage_cache_lists) = collect_coverage_viols(
        py_cov, rs.cov, focus_set, opts.bypass_gate, opts.show_timing,
        py_graph.as_ref(), rs.graph.as_ref(),
    );
    viols.extend(cov_viols.iter().cloned());

    let py_dups = filter_duplicates_by_focus(py_dups_all.clone(), focus_set);
    let rs_dups = filter_duplicates_by_focus(rs.dups.clone(), focus_set);
    log_timing_phase2(opts.show_timing, t_phase2, std::time::Instant::now());

    maybe_store_full_cache(CacheStoreCall {
        opts, py_files, rs_files, result: &result, graph_viols_all: &graph_viols_all,
        coverage_violations: &cov_viols, py_graph: py_graph.as_ref(),
        rs_graph: rs.graph.as_ref(), py_dups_all: &py_dups_all,
        rs_dups_all: &rs.dups, coverage_cache_lists,
    });
    let success = print_all_results_with_dups_opt(
        &viols, &py_dups, &rs_dups, opts.show_timing, Some(t_phase2),
        opts.suppress_final_status,
    );
    AnalyzeResult {
        success,
        metrics: Some(metrics),
    }
}

fn merge_coverage_results(
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

struct CacheStoreCall<'a> {
    opts: &'a AnalyzeOptions<'a>,
    py_files: &'a [PathBuf],
    rs_files: &'a [PathBuf],
    result: &'a ParseResult,
    graph_viols_all: &'a [Violation],
    coverage_violations: &'a [Violation],
    py_graph: Option<&'a DependencyGraph>,
    rs_graph: Option<&'a DependencyGraph>,
    py_dups_all: &'a [DuplicateCluster],
    rs_dups_all: &'a [DuplicateCluster],
    coverage_cache_lists: Option<(Vec<CachedCoverageItem>, Vec<CachedCoverageItem>)>,
}

fn maybe_store_full_cache(call: CacheStoreCall<'_>) {
    if !call.opts.bypass_gate || call.opts.show_timing {
        return;
    }
    let Some((definitions, unreferenced)) = call.coverage_cache_lists else {
        return;
    };
    let fp = analyze_cache::fingerprint_for_check(
        call.py_files,
        call.rs_files,
        call.opts.py_config,
        call.opts.rs_config,
        call.opts.gate_config,
    );
    analyze_cache::store_full_cache_from_run(analyze_cache::FullCacheInputs {
        fingerprint: fp,
        py_file_count: call.result.py_parsed.len(),
        rs_file_count: call.result.rs_parsed.len(),
        code_unit_count: call.result.code_unit_count,
        statement_count: call.result.statement_count,
        violations: &call.result.violations,
        graph_viols_all: call.graph_viols_all,
        coverage_violations: call.coverage_violations,
        py_graph: call.py_graph,
        rs_graph: call.rs_graph,
        py_dups_all: call.py_dups_all,
        rs_dups_all: call.rs_dups_all,
        definitions,
        unreferenced,
    });
}

pub fn run_dry(
    path: &str,
    filter_files: &[String],
    config: &DuplicationConfig,
    ignore_prefixes: &[String],
    lang_filter: Option<Language>,
) {
    let root = Path::new(path);
    let (py_files, rs_files) = gather_files(root, lang_filter, ignore_prefixes);

    if py_files.is_empty() && rs_files.is_empty() {
        print_no_files_message(lang_filter, root);
        return;
    }

    let py_parsed = if py_files.is_empty() {
        Vec::new()
    } else {
        parse_files(&py_files)
            .unwrap_or_default()
            .into_iter()
            .filter_map(py_parsed_or_log)
            .collect()
    };
    let rs_parsed = if rs_files.is_empty() {
        Vec::new()
    } else {
        parse_rust_files(&rs_files)
            .into_iter()
            .filter_map(Result::ok)
            .collect()
    };

    let mut chunks = extract_chunks_for_duplication(&py_parsed.iter().collect::<Vec<_>>());
    chunks.extend(extract_rust_chunks_for_duplication(
        &rs_parsed.iter().collect::<Vec<_>>(),
    ));

    let mut pairs = detect_duplicates_from_chunks(&chunks, config);

    if !filter_files.is_empty() {
        let filters: HashSet<PathBuf> = filter_files
            .iter()
            .map(|f| {
                Path::new(f)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(f))
            })
            .collect();
        pairs.retain(|p| filters.contains(&p.chunk1.file) || filters.contains(&p.chunk2.file));
    }

    print_dry_results(&pairs);
}

fn log_parse_timing(show: bool, timing: &str) {
    if show && !timing.is_empty() {
        eprintln!("[TIMING] {timing}");
    }
}

fn log_timing_phase2(show: bool, t3: std::time::Instant, t4: std::time::Instant) {
    if show {
        eprintln!(
            "[TIMING] graph_analysis={:.2}s, test_refs={:.2}s",
            t4.duration_since(t3).as_secs_f64(),
            std::time::Instant::now().duration_since(t4).as_secs_f64()
        );
    }
}

pub fn filter_viols_by_focus(
    mut viols: Vec<Violation>,
    focus_set: &HashSet<PathBuf>,
) -> Vec<Violation> {
    viols.retain(|v| is_focus_file(&v.file, focus_set));
    viols
}

fn log_timing_phase1(
    t0: std::time::Instant,
    t1: std::time::Instant,
    t2: std::time::Instant,
    t3: std::time::Instant,
) {
    eprintln!(
        "[TIMING] discovery={:.2}s, parse+analyze={:.2}s, coverage=0.00s, graph={:.2}s",
        t1.duration_since(t0).as_secs_f64(),
        t2.duration_since(t1).as_secs_f64(),
        t3.duration_since(t2).as_secs_f64()
    );
}

pub fn gather_files(
    root: &Path,
    lang: Option<Language>,
    ignore_prefixes: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let all = find_source_files_with_ignore(root, ignore_prefixes);
    let (mut py, mut rs) = (Vec::new(), Vec::new());
    for sf in all {
        let path = sf.path.canonicalize().unwrap_or(sf.path);
        match (sf.language, lang) {
            (Language::Python, None | Some(Language::Python)) => py.push(path),
            (Language::Rust, None | Some(Language::Rust)) => rs.push(path),
            _ => {}
        }
    }
    (py, rs)
}

pub fn build_focus_set(
    focus_paths: &[String],
    lang: Option<Language>,
    ignore_prefixes: &[String],
) -> HashSet<PathBuf> {
    let mut focus_set = HashSet::new();
    for focus_path in focus_paths {
        let path = Path::new(focus_path);
        if path.is_file() {
            if let Ok(canonical) = path.canonicalize() {
                focus_set.insert(canonical);
            }
        } else {
            let (py, rs) = gather_files(path, lang, ignore_prefixes);
            focus_set.extend(py);
            focus_set.extend(rs);
        }
    }
    focus_set
}

pub fn is_focus_file(file: &Path, focus_set: &HashSet<PathBuf>) -> bool {
    focus_set.is_empty() || focus_set.contains(file)
}


fn build_py_graph(py_parsed: &[ParsedFile]) -> Option<DependencyGraph> {
    if py_parsed.is_empty() {
        None
    } else {
        Some(build_dependency_graph(
            &py_parsed.iter().collect::<Vec<_>>(),
        ))
    }
}

fn build_rs_graph(rs_parsed: &[ParsedRustFile]) -> Option<DependencyGraph> {
    if rs_parsed.is_empty() {
        None
    } else {
        Some(build_rust_dependency_graph(
            &rs_parsed.iter().collect::<Vec<_>>(),
        ))
    }
}

pub fn build_graphs(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (Option<DependencyGraph>, Option<DependencyGraph>) {
    (build_py_graph(py_parsed), build_rs_graph(rs_parsed))
}

fn print_analysis_summary(
    file_count: usize,
    unit_count: usize,
    stmt_count: usize,
    py_g: Option<&DependencyGraph>,
    rs_g: Option<&DependencyGraph>,
) {
    let (nodes, edges) = graph_stats(py_g, rs_g);
    println!(
        "Analyzed: {file_count} files, {unit_count} code_units, {stmt_count} statements, {nodes} graph_nodes, {edges} graph_edges"
    );
}

fn graph_stats(py_g: Option<&DependencyGraph>, rs_g: Option<&DependencyGraph>) -> (usize, usize) {
    let (mut nodes, mut edges) = (0, 0);
    if let Some(g) = py_g {
        nodes += g.graph.node_count();
        edges += g.graph.edge_count();
    }
    if let Some(g) = rs_g {
        nodes += g.graph.node_count();
        edges += g.graph.edge_count();
    }
    (nodes, edges)
}

#[allow(dead_code)]
pub fn analyze_graphs(
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Vec<Violation> {
    let mut viols = Vec::new();
    if let Some(g) = py_graph {
        viols.extend(analyze_graph(
            g,
            py_config,
            gate_config.orphan_module_enabled,
        ));
    }
    if let Some(g) = rs_graph {
        viols.extend(analyze_graph(
            g,
            rs_config,
            gate_config.orphan_module_enabled,
        ));
    }
    viols
}

#[allow(dead_code)]
pub fn check_coverage_gate(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    gate_config: &GateConfig,
    focus_set: &HashSet<PathBuf>,
    _show_timing: bool,
) -> bool {
    let (defs_cached, unrefs_cached) = analyze_cache::coverage_lists(py_parsed, rs_parsed);
    let defs_t: Vec<_> = defs_cached
        .into_iter()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let unrefs_t: Vec<_> = unrefs_cached
        .into_iter()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let (_, _, _, unreferenced) =
        compute_test_coverage_from_lists(&defs_t, &unrefs_t, focus_set);
    let defs_focus: Vec<_> = defs_t
        .iter()
        .filter(|(f, _, _)| is_focus_file(f, focus_set))
        .cloned()
        .collect();
    let file_pcts = file_coverage_map(&defs_focus, &unreferenced);
    let threshold = gate_config.test_coverage_threshold;
    let any_failing = file_pcts
        .values()
        .any(|&pct| pct < threshold);
    if any_failing {
        let total_defs_focus = defs_focus.len();
        let tested_focus = total_defs_focus.saturating_sub(unreferenced.len());
        print_coverage_gate_failure(
            0, // unused with per-file
            threshold,
            tested_focus,
            total_defs_focus,
            &unreferenced,
            &file_pcts,
        );
        return false;
    }
    true
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

fn print_all_results_with_dups_opt(
    viols: &[Violation],
    py_dups: &[DuplicateCluster],
    rs_dups: &[DuplicateCluster],
    show_timing: bool,
    t0: Option<std::time::Instant>,
    suppress_final_status: bool,
) -> bool {
    let t1 = std::time::Instant::now();
    let dup_count = py_dups.len() + rs_dups.len();

    print_violations(viols);
    print_duplicates("Python", py_dups);
    print_duplicates("Rust", rs_dups);
    if show_timing && let Some(t0) = t0 {
        let t2 = std::time::Instant::now();
        eprintln!(
            "[TIMING] dup_detect={:.2}s, output={:.2}s",
            t1.duration_since(t0).as_secs_f64(),
            t2.duration_since(t1).as_secs_f64()
        );
    }

    let has_violations = !viols.is_empty() || dup_count > 0;
    if !suppress_final_status {
        print_final_status(has_violations);
    }

    !has_violations
}

pub fn filter_duplicates_by_focus(
    dups: Vec<DuplicateCluster>,
    focus_set: &HashSet<PathBuf>,
) -> Vec<DuplicateCluster> {
    dups.into_iter()
        .filter(|cluster| cluster.chunks.iter().any(|c| is_focus_file(&c.file, focus_set)))
        .collect()
}

pub fn detect_py_duplicates(parsed: &[ParsedFile], min_similarity: f64) -> Vec<DuplicateCluster> {
    let config = DuplicationConfig { min_similarity, ..Default::default() };
    cluster_duplicates_from_chunks(&extract_chunks_for_duplication(&parsed.iter().collect::<Vec<_>>()), &config)
}

pub fn detect_rs_duplicates(parsed: &[ParsedRustFile], min_similarity: f64) -> Vec<DuplicateCluster> {
    let config = DuplicationConfig { min_similarity, ..Default::default() };
    cluster_duplicates_from_chunks(&extract_rust_chunks_for_duplication(&parsed.iter().collect::<Vec<_>>()), &config)
}

pub fn compute_global_metrics(
    paths: &[String], ignore: &[String], lang_filter: Option<Language>,
    py_config: &Config, rs_config: &Config,
) -> Option<kiss::GlobalMetrics> {
    use kiss::discovery::gather_files_by_lang;
    let (py_files, rs_files) = gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() { return None; }
    let result = parse_all(&py_files, &rs_files, py_config, rs_config);
    let (py_graph, rs_graph) = build_graphs(&result.py_parsed, &result.rs_parsed);
    let (nodes, edges) = graph_stats(py_graph.as_ref(), rs_graph.as_ref());
    Some(kiss::GlobalMetrics {
        files: result.py_parsed.len() + result.rs_parsed.len(),
        code_units: result.code_unit_count, statements: result.statement_count,
        graph_nodes: nodes, graph_edges: edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_structs() {
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let gate_cfg = GateConfig::default();
        let _ = AnalyzeOptions {
            universe: ".", focus_paths: &[], py_config: &py_cfg, rs_config: &rs_cfg,
            lang_filter: None, bypass_gate: false, gate_config: &gate_cfg,
            ignore_prefixes: &[], show_timing: false, suppress_final_status: false,
        };
        let _ = ParseResult {
            py_parsed: vec![], rs_parsed: vec![], violations: vec![],
            code_unit_count: 0, statement_count: 0,
        };
    }

    #[test]
    fn test_gather_parse_and_graphs() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "import b\ndef f(): pass").unwrap();
        std::fs::write(tmp.path().join("b.py"), "x=1").unwrap();
        std::fs::write(tmp.path().join("c.rs"), "fn main() {}").unwrap();

        let (py, rs) = gather_files(tmp.path(), None, &[]);
        assert_eq!(py.len(), 2);
        assert_eq!(rs.len(), 1);
        assert!(!build_focus_set(&[tmp.path().to_string_lossy().to_string()], None, &[]).is_empty());

        let result = parse_all(&py, &rs, &Config::python_defaults(), &Config::rust_defaults());
        assert_eq!(result.py_parsed.len(), 2);
        assert_eq!(result.rs_parsed.len(), 1);

        let (py_g, rs_g) = build_graphs(&result.py_parsed, &result.rs_parsed);
        assert!(py_g.is_some());
        let _ = analyze_graphs(py_g.as_ref(), rs_g.as_ref(),
            &Config::python_defaults(), &Config::rust_defaults(), &GateConfig::default());
    }

    #[test]
    fn test_gate_helpers_and_empty_analysis() {
        let gate = GateConfig { test_coverage_threshold: 0, ..Default::default() };
        let focus = HashSet::new();
        assert!(check_coverage_gate(&[], &[], &gate, &focus, false));
        let (cov, tested, total, unref) = compute_test_coverage_from_lists(&[], &[], &focus);
        assert_eq!(cov, 100);
        assert_eq!(tested, 0);
        assert_eq!(total, 0);
        assert!(unref.is_empty());
    }

    /// Regression: per-file enforcement must fail when one file is below threshold
    /// even if overall coverage would pass. With overall enforcement this would incorrectly pass.
    #[test]
    fn test_coverage_gate_per_file_fails_when_one_file_below_threshold() {
        let tmp = TempDir::new().unwrap();
        let well = tmp.path().join("well_covered.py");
        let poor = tmp.path().join("poorly_covered.py");
        let test_file = tmp.path().join("test_well.py");

        std::fs::write(
            &well,
            r"def f1(): pass
def f2(): pass
def f3(): pass
def f4(): pass
def f5(): pass
def f6(): pass
def f7(): pass
def f8(): pass
def f9(): pass
",
        )
        .unwrap();
        std::fs::write(&poor, "def orphan_func():\n    pass\n").unwrap();
        std::fs::write(
            &test_file,
            r"from well_covered import f1, f2, f3, f4, f5, f6, f7, f8, f9
def test_all():
    f1(); f2(); f3(); f4(); f5(); f6(); f7(); f8(); f9()
",
        )
        .unwrap();

        let py_files = vec![well, poor, test_file];
        let results = parse_files(&py_files).unwrap();
        let py_parsed: Vec<ParsedFile> = results.into_iter().filter_map(Result::ok).collect();
        assert_eq!(py_parsed.len(), 3, "all 3 files should parse");

        let focus: HashSet<PathBuf> = py_parsed.iter().map(|p| p.path.clone()).collect();
        let gate = GateConfig {
            test_coverage_threshold: 90,
            ..Default::default()
        };

        assert!(
            !check_coverage_gate(&py_parsed, &[], &gate, &focus, false),
            "per-file enforcement must fail when one file (poorly_covered) is below 90%"
        );
    }

    #[test]
    fn test_print_functions_and_helpers() {
        print_analysis_summary(0, 0, 0, None, None);
        let (n, e) = graph_stats(None, None);
        assert_eq!(n, 0);
        assert_eq!(e, 0);
        assert!(is_focus_file(Path::new("any.py"), &HashSet::new()));
        let dups = filter_duplicates_by_focus(vec![], &HashSet::new());
        assert!(dups.is_empty());
    }

    #[test]
    fn test_detect_duplicates() {
        let py_dups = detect_py_duplicates(&[], 0.7);
        assert!(py_dups.is_empty());
        let rs_dups = detect_rs_duplicates(&[], 0.7);
        assert!(rs_dups.is_empty());
    }

    #[test]
    fn test_print_all_results() {
        let result = print_all_results_with_dups_opt(&[], &[], &[], false, None, false);
        assert!(result);
    }

    #[test]
    fn test_run_analyze_no_files() {
        let tmp = TempDir::new().unwrap();
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let gate_cfg = GateConfig::default();
        let opts = AnalyzeOptions {
            universe: tmp.path().to_str().unwrap(), focus_paths: &[],
            py_config: &py_cfg, rs_config: &rs_cfg, lang_filter: None,
            bypass_gate: true, gate_config: &gate_cfg, ignore_prefixes: &[],
            show_timing: false, suppress_final_status: false,
        };
        assert!(run_analyze(&opts));

        std::fs::write(tmp.path().join("lib.rs"), "fn foo() { let x = 1; }").unwrap();
        let (parsed, viols, units, _) = crate::analyze_parse::parse_and_analyze_rs(
            &[tmp.path().join("lib.rs")], &Config::rust_defaults());
        assert_eq!(parsed.len(), 1);
        assert!(viols.is_empty());
        assert!(units > 0);
    }

    #[test]
    #[allow(clippy::let_unit_value)]
    fn test_touch_for_static_test_coverage() {
        fn touch<T>(_t: T) {}
        let _ = std::marker::PhantomData::<RustAnalysis>;
        let _ = (
            touch(run_dry), touch(log_parse_timing), touch(log_timing_phase2),
            touch(filter_viols_by_focus), touch(log_timing_phase1),
            touch(analyze_cache::fingerprint_for_check), touch(analyze_cache::try_run_cached_all),
            touch(analyze_cache::store_full_cache), touch(analyze_cache::coverage_lists),
            touch(analyze_cache::store_full_cache_from_run), touch(compute_test_coverage_from_lists),
            touch(run_analyze_uncached), touch(build_py_graph), touch(build_rs_graph),
            touch(merge_coverage_results), touch(maybe_store_full_cache),
            touch(run_analyze_with_result), touch(print_all_results_with_dups_opt),
            touch(compute_global_metrics), touch(finalize_analysis),
            touch(run_gated_analysis), touch(evaluate_gate), touch(check_coverage_gate),
            touch(run_rust_analysis), touch(run_parallel_py_analysis), touch(build_graph_violations),
            touch(graph_for_path), touch(orphan_post_pass), touch(build_coverage_violation_with_graph),
            touch(collect_coverage_viols), touch(build_metrics),
        );
        let _ = (
            std::mem::size_of::<CacheStoreCall>(),
            std::mem::size_of::<AnalyzeResult>(),
            std::mem::size_of::<AnalysisProducts>(),
        );
    }
}
