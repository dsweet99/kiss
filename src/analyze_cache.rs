use crate::analyze::{
    compute_test_coverage_from_lists, filter_duplicates_by_focus, filter_viols_by_focus,
};
use kiss::check_cache;
use kiss::check_cache::{CachedCodeChunk, CachedViolation};
use kiss::check_universe_cache::{CachedCoverageItem, CachedDuplicateCluster, FullCheckCache};
use kiss::stats::MetricStats;
use kiss::cli_output::{print_duplicates, print_final_status, print_violations};
use kiss::{Config, DuplicateCluster, GateConfig, Violation};
use kiss::{DependencyGraph, ParsedFile, ParsedRustFile};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

mod path_helpers;
use path_helpers::{cache_path_full, load_full_cache, same_cached_paths};

const CACHE_SCHEMA_VERSION: &str = "v2";

pub fn fnv1a64(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn fingerprint_for_check(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv1a64(h, CACHE_SCHEMA_VERSION.as_bytes());
    h = fnv1a64(h, format!("{py_config:?}").as_bytes());
    h = fnv1a64(h, format!("{rs_config:?}").as_bytes());
    h = fnv1a64(h, format!("{gate_config:?}").as_bytes());

    let mut all_files: Vec<&PathBuf> = py_files.iter().chain(rs_files).collect();
    all_files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));

    for p in &all_files {
        h = fnv1a64(h, p.to_string_lossy().as_bytes());
        if let Ok(meta) = std::fs::metadata(p) {
            h = fnv1a64(h, meta.len().to_le_bytes().as_slice());
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| {
                    u128::from(d.as_secs()) * 1_000_000_000_u128 + u128::from(d.subsec_nanos())
                });
            h = fnv1a64(h, mtime_ns.to_le_bytes().as_slice());
        }
    }
    format!("{h:016x}")
}


pub fn store_full_cache(cache: &FullCheckCache) {
    let dir = check_cache::cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(bytes) = bincode::serialize(cache) else {
        return;
    };
    let _ = std::fs::write(cache_path_full(&cache.fingerprint), bytes);
}

pub fn coverage_violation(file: PathBuf, name: String, line: usize, file_pct: usize) -> Violation {
    Violation {
        file,
        line,
        unit_name: name,
        metric: "test_coverage".to_string(),
        value: 0,
        threshold: 0,
        message: format!("{file_pct}% covered. Add test coverage for this code unit."),
        suggestion: String::new(),
    }
}

fn cached_duplicates(
    cache: FullCheckCache,
    gate_config: &GateConfig,
    focus_set: &HashSet<PathBuf>,
) -> (
    Vec<Violation>,
    Vec<DuplicateCluster>,
    Vec<DuplicateCluster>,
    FullCheckCache,
) {
    let mut viols: Vec<Violation> = cache
        .base_violations
        .iter()
        .map(|v| v.clone().into_violation())
        .collect();
    viols.extend(
        cache
            .graph_violations
            .iter()
            .map(|v| v.clone().into_violation()),
    );
    let viols = filter_viols_by_focus(viols, focus_set);

    let (py_dups, rs_dups) = if gate_config.duplication_enabled {
        (
            filter_duplicates_by_focus(
                cache
                    .py_duplicates
                    .iter()
                    .map(|c| DuplicateCluster {
                        avg_similarity: c.avg_similarity,
                        chunks: c.chunks.iter().map(|cc| cc.clone().into_chunk()).collect(),
                    })
                    .collect(),
                focus_set,
            ),
            filter_duplicates_by_focus(
                cache
                    .rs_duplicates
                    .iter()
                    .map(|c| DuplicateCluster {
                        avg_similarity: c.avg_similarity,
                        chunks: c.chunks.iter().map(|cc| cc.clone().into_chunk()).collect(),
                    })
                    .collect(),
                focus_set,
            ),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    (viols, py_dups, rs_dups, cache)
}

fn cached_coverage_viols(cache: &FullCheckCache, focus_set: &HashSet<PathBuf>) -> Vec<Violation> {
    let defs: Vec<_> = cache
        .definitions
        .iter()
        .cloned()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let unref: Vec<_> = cache
        .unreferenced
        .iter()
        .cloned()
        .map(CachedCoverageItem::into_tuple)
        .collect();
    let (_, _, _, unreferenced) = compute_test_coverage_from_lists(&defs, &unref, focus_set);

    if !cache.coverage_violations.is_empty() {
        let lookup: std::collections::HashMap<(String, String, usize), Violation> = cache
            .coverage_violations
            .iter()
            .map(|v| {
                (
                    (v.file.clone(), v.unit_name.clone(), v.line),
                    v.clone().into_violation(),
                )
            })
            .collect();
        return unreferenced
            .into_iter()
            .filter_map(|(file, name, line)| {
                let key = (file.to_string_lossy().to_string(), name, line);
                lookup.get(&key).cloned()
            })
            .collect();
    }

    let file_pcts = kiss::cli_output::file_coverage_map(&defs, &unreferenced);
    unreferenced
        .into_iter()
        .map(|(file, name, line)| {
            let pct = file_pcts.get(&file).copied().unwrap_or(0);
            coverage_violation(file, name, line, pct)
        })
        .collect()
}

pub fn try_run_cached_all(
    opts: &crate::analyze::AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Option<bool> {
    let fp = fingerprint_for_check(
        py_files,
        rs_files,
        opts.py_config,
        opts.rs_config,
        opts.gate_config,
    );
    let cache = load_full_cache(&fp)?;
    if !same_cached_paths(py_files, rs_files, focus_set, &cache) {
        return None;
    }
    if !opts.bypass_gate
        && opts.gate_config.test_coverage_threshold > 0
        && crate::analyze::evaluate_cached_gate(
            &cache.definitions,
            &cache.unreferenced,
            focus_set,
            opts.gate_config.test_coverage_threshold,
        )
        .is_some()
    {
        return Some(false);
    }
    let (mut viols, py_dups, rs_dups, cache) =
        cached_duplicates(cache, opts.gate_config, focus_set);
    viols.extend(cached_coverage_viols(&cache, focus_set));

    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        cache.py_file_count + cache.rs_file_count,
        cache.code_unit_count,
        cache.statement_count,
        cache.graph_nodes,
        cache.graph_edges
    );
    print_violations(&viols);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    let has_violations = !(viols.is_empty() && py_dups.is_empty() && rs_dups.is_empty());
    print_final_status(has_violations);
    Some(!has_violations)
}

pub(crate) fn try_run_cached_stats_summary(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> Option<FullCheckCache> {
    let fp = fingerprint_for_check(py_files, rs_files, py_config, rs_config, gate_config);
    let cache = load_full_cache(&fp)?;
    let focus_set: HashSet<PathBuf> = py_files.iter().chain(rs_files).cloned().collect();
    if !same_cached_paths(py_files, rs_files, &focus_set, &cache) {
        return None;
    }
    if cache.py_file_count > 0 && cache.py_stats.is_none() {
        return None;
    }
    if cache.rs_file_count > 0 && cache.rs_stats.is_none() {
        return None;
    }
    Some(cache)
}

pub fn graph_counts(
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
) -> (usize, usize) {
    let nodes = py_graph.as_ref().map_or(0, |g| g.graph.node_count())
        + rs_graph.as_ref().map_or(0, |g| g.graph.node_count());
    let edges = py_graph.as_ref().map_or(0, |g| g.graph.edge_count())
        + rs_graph.as_ref().map_or(0, |g| g.graph.edge_count());
    (nodes, edges)
}

#[allow(dead_code)]
pub fn coverage_lists(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>) {
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let py_cov = kiss::analyze_test_refs_quick(&py_refs);
    let rs_cov = kiss::analyze_rust_test_refs(&rs_refs, None);

    let to_cached = |file: PathBuf, name: String, line: usize| CachedCoverageItem {
        file: file.to_string_lossy().to_string(),
        name,
        line,
    };

    let mut definitions: Vec<CachedCoverageItem> = py_cov.definitions.into_iter()
        .map(|d| to_cached(d.file, d.name, d.line)).collect();
    definitions.extend(rs_cov.definitions.into_iter()
        .map(|d| to_cached(d.file, d.name, d.line)));
    let mut unreferenced: Vec<CachedCoverageItem> = py_cov.unreferenced.into_iter()
        .map(|d| to_cached(d.file, d.name, d.line)).collect();
    unreferenced.extend(rs_cov.unreferenced.into_iter()
        .map(|d| to_cached(d.file, d.name, d.line)));
    (definitions, unreferenced)
}

pub struct FullCacheInputs<'a> {
    pub fingerprint: String,
    pub py_file_count: usize,
    pub rs_file_count: usize,
    pub code_unit_count: usize,
    pub statement_count: usize,
    pub violations: &'a [Violation],
    pub graph_viols_all: &'a [Violation],
    pub coverage_violations: &'a [Violation],
    pub py_graph: Option<&'a DependencyGraph>,
    pub rs_graph: Option<&'a DependencyGraph>,
    pub py_stats: Option<&'a MetricStats>,
    pub rs_stats: Option<&'a MetricStats>,
    pub focus_paths: Vec<String>,
    pub py_paths: Vec<String>,
    pub rs_paths: Vec<String>,
    pub py_dups_all: &'a [DuplicateCluster],
    pub rs_dups_all: &'a [DuplicateCluster],
    pub definitions: Vec<CachedCoverageItem>,
    pub unreferenced: Vec<CachedCoverageItem>,
}

pub fn store_full_cache_from_run(inputs: FullCacheInputs<'_>) {
    let (graph_nodes, graph_edges) = graph_counts(inputs.py_graph, inputs.rs_graph);
    let cache = FullCheckCache {
        fingerprint: inputs.fingerprint,
        py_stats: inputs.py_stats.cloned(),
        rs_stats: inputs.rs_stats.cloned(),
        focus_paths: inputs.focus_paths,
        py_paths: inputs.py_paths,
        rs_paths: inputs.rs_paths,
        py_file_count: inputs.py_file_count,
        rs_file_count: inputs.rs_file_count,
        code_unit_count: inputs.code_unit_count,
        statement_count: inputs.statement_count,
        graph_nodes,
        graph_edges,
        base_violations: inputs
            .violations
            .iter()
            .map(CachedViolation::from)
            .collect(),
        graph_violations: inputs
            .graph_viols_all
            .iter()
            .map(CachedViolation::from)
            .collect(),
        coverage_violations: inputs
            .coverage_violations
            .iter()
            .map(CachedViolation::from)
            .collect(),
        py_duplicates: inputs
            .py_dups_all
            .iter()
            .map(|c| CachedDuplicateCluster {
                avg_similarity: c.avg_similarity,
                chunks: c.chunks.iter().map(CachedCodeChunk::from).collect(),
            })
            .collect(),
        rs_duplicates: inputs
            .rs_dups_all
            .iter()
            .map(|c| CachedDuplicateCluster {
                avg_similarity: c.avg_similarity,
                chunks: c.chunks.iter().map(CachedCodeChunk::from).collect(),
            })
            .collect(),
        definitions: inputs.definitions,
        unreferenced: inputs.unreferenced,
    };
    store_full_cache(&cache);
}

#[cfg(test)]
mod tests;
