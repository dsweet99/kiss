use crate::check_cache::{CachedCodeChunk, CachedViolation};
use crate::stats::MetricStats;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDuplicateCluster {
    pub chunks: Vec<CachedCodeChunk>,
    pub avg_similarity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCoverageItem {
    pub file: String,
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFileCoverage {
    pub file: String,
    pub pct: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCheckCache {
    pub fingerprint: String,
    #[serde(default)]
    pub py_stats: Option<MetricStats>,
    #[serde(default)]
    pub rs_stats: Option<MetricStats>,
    #[serde(default)]
    pub py_paths: Vec<String>,
    #[serde(default)]
    pub focus_paths: Vec<String>,
    #[serde(default)]
    pub focus_restrict: bool,
    #[serde(default)]
    pub rs_paths: Vec<String>,
    pub py_file_count: usize,
    pub rs_file_count: usize,
    pub code_unit_count: usize,
    pub statement_count: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,

    pub base_violations: Vec<CachedViolation>,
    pub graph_violations: Vec<CachedViolation>,
    /// Coverage violations with graph-enhanced messages (fan-in, candidates).
    #[serde(default)]
    pub coverage_violations: Vec<CachedViolation>,

    pub py_duplicates: Vec<CachedDuplicateCluster>,
    pub rs_duplicates: Vec<CachedDuplicateCluster>,

    pub definitions: Vec<CachedCoverageItem>,
    pub unreferenced: Vec<CachedCoverageItem>,
    /// Weighted per-file coverage percentages used by the live gate.
    #[serde(default)]
    pub weighted_file_pcts: Vec<CachedFileCoverage>,
    /// Per-file content digests captured at cache-write time; verified on replay.
    #[serde(default)]
    pub file_content_digests: Vec<(String, u64)>,
}

impl CachedCoverageItem {
    pub fn into_tuple(self) -> (PathBuf, String, usize) {
        (PathBuf::from(self.file), self.name, self.line)
    }
}

impl CachedFileCoverage {
    pub fn into_tuple(self) -> (PathBuf, usize) {
        (PathBuf::from(self.file), self.pct)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn cached_coverage_item_into_tuple_preserves_fields() {
        let item = CachedCoverageItem {
            file: "a.py".to_string(),
            name: "x".to_string(),
            line: 7,
        };
        let (p, name, line) = item.into_tuple();
        assert_eq!(p, PathBuf::from("a.py"));
        assert_eq!(name, "x");
        assert_eq!(line, 7);
    }

    #[test]
    fn cached_duplicate_cluster_deserializes_chunk_fields() {
        let json = serde_json::json!({
            "chunks": [{
                "file": "a.py",
                "name": "x",
                "start_line": 1,
                "end_line": 5,
                "normalized": "n"
            }],
            "avg_similarity": 0.95
        });

        let cluster: CachedDuplicateCluster = serde_json::from_value(json).unwrap();

        assert_eq!(cluster.chunks.len(), 1);
        assert_eq!(cluster.chunks[0].file, "a.py");
        assert_eq!(cluster.avg_similarity, 0.95);
    }

    #[test]
    fn full_check_cache_deserializes_defaulted_recent_fields() {
        let json = serde_json::json!({
            "fingerprint": "deadbeef",
            "py_paths": [],
            "focus_paths": [],
            "focus_restrict": false,
            "rs_paths": [],
            "py_file_count": 0,
            "rs_file_count": 0,
            "code_unit_count": 0,
            "statement_count": 0,
            "graph_nodes": 0,
            "graph_edges": 0,
            "base_violations": [],
            "graph_violations": [],
            "py_duplicates": [],
            "rs_duplicates": [],
            "definitions": [],
            "unreferenced": []
        });

        let cache: FullCheckCache = serde_json::from_value(json).unwrap();

        assert!(cache.coverage_violations.is_empty());
        assert!(cache.weighted_file_pcts.is_empty());
        assert!(cache.file_content_digests.is_empty());
    }

    #[test]
    fn cached_file_coverage_into_tuple_preserves_fields() {
        let item = CachedFileCoverage {
            file: "a.py".to_string(),
            pct: 91,
        };
        let (p, pct) = item.into_tuple();
        assert_eq!(p, PathBuf::from("a.py"));
        assert_eq!(pct, 91);
    }
}
