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
pub struct CachedLineCoverageRecord {
    pub file: String,
    pub total_lines: usize,
    pub covered_lines: usize,
    pub percent: usize,
    pub first_uncovered_line: Option<usize>,
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

    pub py_duplicates: Vec<CachedDuplicateCluster>,
    pub rs_duplicates: Vec<CachedDuplicateCluster>,

    #[serde(default)]
    pub file_content_digests: Vec<(String, u64)>,
}

impl CachedCoverageItem {
    pub fn into_tuple(self) -> (PathBuf, String, usize) {
        (PathBuf::from(self.file), self.name, self.line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_cache::CachedCodeChunk;

    #[test]
    fn test_cached_coverage_item_into_tuple() {
        let item = CachedCoverageItem::witness();
        let (p, name, line) = item.into_tuple();
        assert_eq!(p, PathBuf::from("a.py"));
        assert_eq!(name, "x");
        assert_eq!(line, 7);
    }

    impl CachedCoverageItem {
        fn witness() -> Self {
            Self {
                file: "a.py".to_string(),
                name: "x".to_string(),
                line: 7,
            }
        }
    }

    impl CachedDuplicateCluster {
        fn witness() -> Self {
            Self {
                chunks: vec![CachedCodeChunk {
                    file: "a.py".to_string(),
                    name: "x".to_string(),
                    start_line: 1,
                    end_line: 1,
                    normalized: "n".to_string(),
                }],
                avg_similarity: 1.0,
            }
        }
    }

    impl FullCheckCache {
        fn witness() -> Self {
            Self {
                fingerprint: "deadbeef".to_string(),
                py_stats: None,
                rs_stats: None,
                py_paths: vec![],
                focus_paths: vec![],
                focus_restrict: false,
                rs_paths: vec![],
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
                file_content_digests: Vec::new(),
            }
        }
    }

    #[test]
    fn witness_cache_types() {
        let _ = CachedDuplicateCluster::witness();
        let _ = FullCheckCache::witness();
    }
}
