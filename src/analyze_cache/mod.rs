mod content_digest;
mod emit;

use crate::analyze::FocusFilter;
use crate::analyze::{filter_duplicates_by_focus, filter_viols_by_focus};
use emit::{emit_cached_bypass, emit_cached_gated};
use kiss::check_cache;
use kiss::check_universe_cache::FullCheckCache;
use kiss::{Config, DuplicateCluster, GateConfig, Violation};
use kiss::DependencyGraph;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

mod path_helpers;
mod stats_top;
mod store_full;
#[cfg(test)]
mod test_helpers;
pub(crate) use content_digest::load_verified_full_cache;
use path_helpers::{cache_path_full, same_cached_paths};
pub(crate) use stats_top::{
    try_run_cached_stats_summary,
};
pub use store_full::{FullCacheInputs, store_full_cache_from_run};

const CACHE_SCHEMA_VERSION: &str = "v12-static";

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
    let cache = load_verified_full_cache(&fp, py_files, rs_files)?;
    if !same_cached_paths(py_files, rs_files, focus, &cache) {
        return None;
    }
    if opts.bypass_gate {
        Some(emit_cached_bypass(cache, opts, focus))
    } else {
        Some(emit_cached_gated(cache, opts, focus))
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


#[cfg(test)]
mod coverage_witness {
    use super::*;

    fn fnv1a64_witness() -> u64 {
        fnv1a64(0xcbf2_9ce4_8422_2325_u64, b"witness")
    }

    #[test]
    fn witness_hash_and_full_cache_inputs() {
        let h0 = 0xcbf2_9ce4_8422_2325_u64;
        assert_eq!(fnv1a64(h0, b""), h0);
        assert_ne!(fnv1a64(h0, b"a"), fnv1a64(h0, b"b"));
        assert_eq!(fnv1a64_witness(), fnv1a64(h0, b"witness"));
    }
}

#[cfg(test)]
mod tests;
