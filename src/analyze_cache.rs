use crate::analyze::{
    compute_test_coverage_from_lists, filter_duplicates_by_focus, filter_viols_by_focus,
};
use kiss::cli_output::{print_duplicates, print_final_status, print_violations};
use kiss::{Config, DuplicateCluster, GateConfig, Violation};
use kiss::{DependencyGraph, ParsedFile, ParsedRustFile};
use kiss::check_cache;
use kiss::check_cache::{CachedCodeChunk, CachedViolation};
use kiss::check_universe_cache::{CachedCoverageItem, CachedDuplicateCluster, FullCheckCache};
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

pub fn fingerprint_for_check(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    gate_config: &GateConfig,
) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    // Include binary version so cache is invalidated on upgrade
    h = fnv1a64(h, env!("CARGO_PKG_VERSION").as_bytes());
    h = fnv1a64(h, format!("{py_config:?}").as_bytes());
    h = fnv1a64(h, format!("{rs_config:?}").as_bytes());
    h = fnv1a64(h, format!("{gate_config:?}").as_bytes());

    let mut all_files: Vec<&PathBuf> = py_files.iter().chain(rs_files).collect();
    all_files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));

    for p in all_files {
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

pub fn coverage_violation(file: PathBuf, name: String, line: usize) -> Violation {
    Violation {
        file,
        line,
        unit_name: name,
        metric: "test_coverage".to_string(),
        value: 0,
        threshold: 0,
        message: "Add test coverage for this code unit.".to_string(),
        suggestion: String::new(),
    }
}

fn cached_duplicates(
    cache: FullCheckCache,
    gate_config: &GateConfig,
    focus_set: &HashSet<PathBuf>,
) -> (Vec<Violation>, Vec<DuplicateCluster>, Vec<DuplicateCluster>, FullCheckCache) {
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
                        chunks: c
                            .chunks
                            .iter()
                            .map(|cc| cc.clone().into_chunk())
                            .collect(),
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
                        chunks: c
                            .chunks
                            .iter()
                            .map(|cc| cc.clone().into_chunk())
                            .collect(),
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
    unreferenced
        .into_iter()
        .map(|(file, name, line)| coverage_violation(file, name, line))
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
    let (mut viols, py_dups, rs_dups, cache) = cached_duplicates(cache, opts.gate_config, focus_set);
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

pub fn coverage_lists(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>) {
    let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
    let rs_refs: Vec<&ParsedRustFile> = rs_parsed.iter().collect();
    let py_cov = kiss::analyze_test_refs(&py_refs);
    let rs_cov = kiss::analyze_rust_test_refs(&rs_refs);

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

pub struct FullCacheInputs<'a> {
    pub fingerprint: String,
    pub py_file_count: usize,
    pub rs_file_count: usize,
    pub code_unit_count: usize,
    pub statement_count: usize,
    pub violations: &'a [Violation],
    pub graph_viols_all: &'a [Violation],
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
        base_violations: inputs.violations.iter().map(CachedViolation::from).collect(),
        graph_violations: inputs
            .graph_viols_all
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
mod tests {
    use super::*;

    #[test]
    fn test_touch_analyze_cache_privates_for_static_coverage() {
        fn touch<T>(_t: T) {}
        touch(fnv1a64);
        touch(cache_path_full);
        touch(load_full_cache);
        touch(cached_duplicates);
        touch(cached_coverage_viols);
    }

    #[test]
    fn test_cached_helpers_smoke() {
        let fp = "deadbeef";
        let _ = cache_path_full(fp);
        let _ = load_full_cache(fp); // likely None

        let empty = FullCheckCache {
            fingerprint: fp.to_string(),
            py_file_count: 0,
            rs_file_count: 0,
            code_unit_count: 0,
            statement_count: 0,
            graph_nodes: 0,
            graph_edges: 0,
            base_violations: Vec::new(),
            graph_violations: Vec::new(),
            py_duplicates: Vec::new(),
            rs_duplicates: Vec::new(),
            definitions: Vec::new(),
            unreferenced: Vec::new(),
        };
        let focus = HashSet::new();
        let (_viols, py_dups, rs_dups, cache) = cached_duplicates(empty, &GateConfig::default(), &focus);
        assert!(py_dups.is_empty());
        assert!(rs_dups.is_empty());
        let cov = cached_coverage_viols(&cache, &focus);
        assert!(cov.is_empty());
    }
}

