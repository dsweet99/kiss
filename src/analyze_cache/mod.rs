mod all_replay;
mod content_digest;
mod emit;
mod gate_replay;
use crate::analyze::FocusFilter;
use crate::analyze::{
    build_file_coverage_violation, compute_test_coverage_from_lists, filter_duplicates_by_focus,
    filter_viols_by_focus,
};
use all_replay::{store_all_replay_cache, try_run_cached_all_replay};
use emit::{emit_cached_bypass, emit_cached_gated};
pub(crate) use gate_replay::{store_gate_failure_replay_cache, try_run_cached_gate_failure};
use kiss::check_cache;
use kiss::check_cache::{CachedCodeChunk, CachedViolation};
use kiss::check_universe_cache::{
    CachedCoverageItem, CachedDuplicateCluster, CachedFileCoverage, FullCheckCache,
};
use kiss::stats::MetricStats;
use kiss::{Config, DependencyGraph, DuplicateCluster, GateConfig, Violation};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
mod path_helpers;
mod stats_top;
#[cfg(test)]
pub(crate) mod test_helpers;
pub(crate) use content_digest::load_verified_full_cache;
use path_helpers::{cache_path_full, same_cached_paths};
pub(crate) use stats_top::{
    maybe_store_stats_top_cache, try_run_cached_stats_summary, try_run_cached_stats_top,
};

const CACHE_SCHEMA_VERSION: &str = "v14";

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
    h = fnv1a64(h, CACHE_SCHEMA_VERSION.as_bytes());
    h = fnv1a64(h, env!("CARGO_PKG_VERSION").as_bytes());
    h = mix_config_into_fingerprint(h, py_config);
    h = mix_config_into_fingerprint(h, rs_config);
    h = mix_gate_into_fingerprint(h, gate_config);

    let mut all_files: Vec<&PathBuf> = py_files.iter().chain(rs_files).collect();
    all_files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));

    for p in &all_files {
        h = fnv1a64(h, p.to_string_lossy().as_bytes());
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

#[cfg(test)]
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
    focus: &FocusFilter,
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
    let viols = filter_viols_by_focus(viols, focus);

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
                focus,
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
                focus,
            ),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    (viols, py_dups, rs_dups, cache)
}

fn cached_coverage_viols(cache: &FullCheckCache, focus: &FocusFilter) -> Vec<Violation> {
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
    let (_, _, _, unreferenced) = compute_test_coverage_from_lists(&defs, &unref, focus);

    let weighted = weighted_file_pct_map(&cache.weighted_file_pcts);
    let mut file_pcts = kiss::cli_output::file_coverage_map(&defs, &unreferenced);
    file_pcts.extend(weighted);
    let mut failing_files: BTreeMap<PathBuf, usize> = BTreeMap::new();
    for (file, name, _) in unreferenced {
        if crate::analyze::is_coverage_report_target(&file, &name, true) {
            failing_files
                .entry(file.clone())
                .or_insert_with(|| file_pcts.get(&file).copied().unwrap_or(0));
        }
    }
    failing_files
        .into_iter()
        .map(|(file, pct)| build_file_coverage_violation(file, pct))
        .collect()
}

pub(crate) fn weighted_file_pct_map(items: &[CachedFileCoverage]) -> HashMap<PathBuf, usize> {
    items
        .iter()
        .cloned()
        .map(CachedFileCoverage::into_tuple)
        .collect()
}

pub fn try_run_cached_all(
    opts: &crate::analyze::AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus: &FocusFilter,
) -> Option<bool> {
    let fp = fingerprint_for_check(
        py_files,
        rs_files,
        opts.py_config,
        opts.rs_config,
        opts.gate_config,
    );
    if let Some(ok) = try_run_cached_all_replay(opts, py_files, rs_files, focus, &fp) {
        return Some(ok);
    }
    let cache = load_verified_full_cache(&fp, py_files, rs_files)?;
    if !same_cached_paths(py_files, rs_files, focus, &cache) {
        return None;
    }
    let repo_root = std::path::Path::new(opts.universe);
    if kiss::rslip_bridge::rslip_database_fingerprint(repo_root) != cache.rslip_fingerprint {
        return None;
    }
    if kiss::rust_llvm_cov::backend_fingerprint(repo_root) != cache.rust_coverage_fingerprint {
        return None;
    }

    if opts.bypass_gate {
        store_all_replay_cache(&fp, opts, focus, &cache);
        Some(emit_cached_bypass(cache, opts, focus))
    } else {
        let gate_violations = crate::analyze::gate_failure_violations_from_cached(
            &cache.definitions,
            &cache.unreferenced,
            focus,
            opts.gate_config.test_coverage_threshold,
            None,
        );
        let ok = emit_cached_gated(cache, opts, focus);
        if !ok {
            store_gate_failure_replay_cache(fp, opts, py_files, rs_files, focus, &gate_violations);
        }
        Some(ok)
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
    pub focus_restrict: bool,
    pub py_paths: Vec<String>,
    pub rs_paths: Vec<String>,
    pub py_dups_all: &'a [DuplicateCluster],
    pub rs_dups_all: &'a [DuplicateCluster],
    pub definitions: Vec<CachedCoverageItem>,
    pub unreferenced: Vec<CachedCoverageItem>,
    pub weighted_file_pcts: Vec<CachedFileCoverage>,
    pub rslip_fingerprint: String,
    pub rust_coverage_fingerprint: String,
}

pub fn store_full_cache_from_run(inputs: FullCacheInputs<'_>) -> FullCheckCache {
    let (graph_nodes, graph_edges) = graph_counts(inputs.py_graph, inputs.rs_graph);
    let py_path_bufs: Vec<PathBuf> = inputs.py_paths.iter().map(PathBuf::from).collect();
    let rs_path_bufs: Vec<PathBuf> = inputs.rs_paths.iter().map(PathBuf::from).collect();
    let mut file_content_digests = content_digest::content_digests_for_paths(&py_path_bufs);
    file_content_digests.extend(content_digest::content_digests_for_paths(&rs_path_bufs));
    file_content_digests.sort_by(|a, b| a.0.cmp(&b.0));
    let mut file_metadata_fingerprints =
        content_digest::metadata_fingerprints_for_paths(&py_path_bufs);
    file_metadata_fingerprints.extend(content_digest::metadata_fingerprints_for_paths(
        &rs_path_bufs,
    ));
    file_metadata_fingerprints.sort_by(|a, b| a.0.cmp(&b.0));
    let cache = FullCheckCache {
        fingerprint: inputs.fingerprint,
        py_stats: inputs.py_stats.cloned(),
        rs_stats: inputs.rs_stats.cloned(),
        focus_paths: inputs.focus_paths,
        focus_restrict: inputs.focus_restrict,
        py_paths: inputs.py_paths,
        rs_paths: inputs.rs_paths,
        py_file_count: inputs.py_file_count,
        rs_file_count: inputs.rs_file_count,
        code_unit_count: inputs.code_unit_count,
        statement_count: inputs.statement_count,
        graph_nodes,
        graph_edges,
        file_content_digests,
        file_metadata_fingerprints,
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
        weighted_file_pcts: inputs.weighted_file_pcts,
        rslip_fingerprint: inputs.rslip_fingerprint,
        rust_coverage_fingerprint: inputs.rust_coverage_fingerprint,
    };
    store_full_cache(&cache);
    cache
}
#[cfg(test)]
mod content_digest_test;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_fingerprint;
#[cfg(test)]
mod tests_replay;
