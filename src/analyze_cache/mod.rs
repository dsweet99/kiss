mod emit;

use crate::analyze::{
    compute_test_coverage_from_lists, filter_duplicates_by_focus, filter_viols_by_focus,
};
use emit::{emit_cached_bypass, emit_cached_gated};
use kiss::check_cache;
use kiss::check_cache::{CachedCodeChunk, CachedViolation};
use kiss::check_universe_cache::{CachedCoverageItem, CachedDuplicateCluster, FullCheckCache};
use kiss::{Config, DuplicateCluster, GateConfig, Violation};
use kiss::{DependencyGraph, ParsedFile, ParsedRustFile};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub fn fnv1a64(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn mix_config_into_fingerprint(mut h: u64, cfg: &Config) -> u64 {
    for u in [
        cfg.statements_per_function,
        cfg.methods_per_class,
        cfg.statements_per_file,
        cfg.lines_per_file,
        cfg.functions_per_file,
        cfg.arguments_per_function,
        cfg.arguments_positional,
        cfg.arguments_keyword_only,
        cfg.max_indentation_depth,
        cfg.interface_types_per_file,
        cfg.concrete_types_per_file,
        cfg.nested_function_depth,
        cfg.returns_per_function,
        cfg.return_values_per_function,
        cfg.branches_per_function,
        cfg.local_variables_per_function,
        cfg.imported_names_per_file,
        cfg.statements_per_try_block,
        cfg.boolean_parameters,
        cfg.annotations_per_function,
        cfg.calls_per_function,
        cfg.cycle_size,
        cfg.indirect_dependencies,
        cfg.dependency_depth,
    ] {
        h = fnv1a64(h, u.to_le_bytes().as_slice());
    }
    h
}

fn mix_gate_into_fingerprint(mut h: u64, gate: &GateConfig) -> u64 {
    h = fnv1a64(h, gate.test_coverage_threshold.to_le_bytes().as_slice());
    h = fnv1a64(h, gate.min_similarity.to_bits().to_le_bytes().as_slice());
    h = fnv1a64(h, &[u8::from(gate.duplication_enabled)]);
    h = fnv1a64(h, &[u8::from(gate.orphan_module_enabled)]);
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
    h = fnv1a64(h, env!("CARGO_PKG_VERSION").as_bytes());
    h = mix_config_into_fingerprint(h, py_config);
    h = mix_config_into_fingerprint(h, rs_config);
    h = mix_gate_into_fingerprint(h, gate_config);

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

fn cache_path_full(fingerprint: &str) -> PathBuf {
    check_cache::cache_dir().join(format!("check_full_{fingerprint}.bin"))
}

fn load_full_cache(fingerprint: &str) -> Option<FullCheckCache> {
    let p = cache_path_full(fingerprint);
    let bytes = std::fs::read(p).ok()?;
    let c: FullCheckCache = bincode::deserialize(&bytes).ok()?;
    (c.fingerprint == fingerprint).then_some(c)
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

    if opts.bypass_gate {
        Some(emit_cached_bypass(cache, opts, focus_set))
    } else {
        Some(emit_cached_gated(cache, opts, focus_set))
    }
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

    let mut definitions: Vec<CachedCoverageItem> = py_cov
        .definitions
        .into_iter()
        .map(|d| to_cached(d.file, d.name, d.line))
        .collect();
    definitions.extend(
        rs_cov
            .definitions
            .into_iter()
            .map(|d| to_cached(d.file, d.name, d.line)),
    );
    let mut unreferenced: Vec<CachedCoverageItem> = py_cov
        .unreferenced
        .into_iter()
        .map(|d| to_cached(d.file, d.name, d.line))
        .collect();
    unreferenced.extend(
        rs_cov
            .unreferenced
            .into_iter()
            .map(|d| to_cached(d.file, d.name, d.line)),
    );
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
    pub py_dups_all: &'a [DuplicateCluster],
    pub rs_dups_all: &'a [DuplicateCluster],
    pub definitions: Vec<CachedCoverageItem>,
    pub unreferenced: Vec<CachedCoverageItem>,
}

pub fn store_full_cache_from_run(inputs: FullCacheInputs<'_>) {
    let (graph_nodes, graph_edges) = graph_counts(inputs.py_graph, inputs.rs_graph);
    let cache = FullCheckCache {
        fingerprint: inputs.fingerprint,
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
