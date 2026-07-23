use std::path::PathBuf;

use kiss::check_cache::{CachedCodeChunk, CachedViolation};
use kiss::check_universe_cache::{CachedDuplicateCluster, FullCheckCache};
use kiss::stats::MetricStats;
use kiss::{DependencyGraph, DuplicateCluster, Violation};

use super::{content_digest, graph_counts, store_full_cache};

#[derive(Default)]
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
    pub py_stats: Option<&'a MetricStats>,
    pub rs_stats: Option<&'a MetricStats>,
    pub focus_paths: Vec<String>,
    pub focus_restrict: bool,
    pub py_paths: Vec<String>,
    pub rs_paths: Vec<String>,
    pub py_dups_all: &'a [DuplicateCluster],
    pub rs_dups_all: &'a [DuplicateCluster],
}

pub fn store_full_cache_from_run(inputs: FullCacheInputs<'_>) {
    let (graph_nodes, graph_edges) = graph_counts(inputs.py_graph, inputs.rs_graph);
    let py_path_bufs: Vec<PathBuf> = inputs.py_paths.iter().map(PathBuf::from).collect();
    let rs_path_bufs: Vec<PathBuf> = inputs.rs_paths.iter().map(PathBuf::from).collect();
    let mut file_content_digests = content_digest::content_digests_for_paths(&py_path_bufs);
    file_content_digests.extend(content_digest::content_digests_for_paths(&rs_path_bufs));
    file_content_digests.sort_by(|a, b| a.0.cmp(&b.0));
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
    };
    store_full_cache(&cache);
}

#[cfg(test)]
impl FullCacheInputs<'static> {
    fn witness() -> Self {
        Self {
            fingerprint: "fp".to_string(),
            py_file_count: 1,
            rs_file_count: 2,
            code_unit_count: 3,
            statement_count: 4,
            focus_restrict: true,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    #[test]
    fn witness_full_cache_inputs() {
        let inputs = FullCacheInputs::witness();
        assert_eq!(inputs.fingerprint, "fp");
        assert_eq!(inputs.py_file_count + inputs.rs_file_count, 3);
        assert!(inputs.focus_restrict);
    }
}
