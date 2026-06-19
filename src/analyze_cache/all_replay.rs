use crate::analyze::FocusFilter;
use kiss::check_cache::{CachedCodeChunk, CachedViolation};
use kiss::check_universe_cache::{CachedDuplicateCluster, FullCheckCache};
use kiss::{DuplicateCluster, check_cache};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{cached_coverage_viols, cached_duplicates, content_digest};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllReplayCache {
    fingerprint: String,
    py_paths: Vec<String>,
    rs_paths: Vec<String>,
    focus_paths: Vec<String>,
    focus_restrict: bool,
    py_file_count: usize,
    rs_file_count: usize,
    code_unit_count: usize,
    statement_count: usize,
    graph_nodes: usize,
    graph_edges: usize,
    violations: Vec<CachedViolation>,
    py_duplicates: Vec<CachedDuplicateCluster>,
    rs_duplicates: Vec<CachedDuplicateCluster>,
    file_content_digests: Vec<(String, u64)>,
    file_metadata_fingerprints: Vec<(String, u64)>,
    rslip_fingerprint: String,
    rust_coverage_fingerprint: String,
}

fn cache_path_all_replay(fingerprint: &str) -> PathBuf {
    check_cache::cache_dir().join(format!("check_all_replay_{fingerprint}.bin"))
}

fn same_all_replay_paths(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus: &FocusFilter,
    cache: &AllReplayCache,
) -> bool {
    let mut py_now: Vec<_> = py_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let mut rs_now: Vec<_> = rs_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let mut py_cached = cache.py_paths.clone();
    let mut rs_cached = cache.rs_paths.clone();
    py_now.sort();
    rs_now.sort();
    py_cached.sort();
    rs_cached.sort();
    if py_now != py_cached || rs_now != rs_cached {
        return false;
    }
    if cache.focus_restrict != focus.is_active() {
        return false;
    }
    if !cache.focus_restrict {
        return true;
    }
    let mut focus_cached = cache.focus_paths.clone();
    focus_cached.sort();
    focus.cache_focus_paths() == focus_cached
}

fn load_all_replay_cache(fingerprint: &str) -> Option<AllReplayCache> {
    let bytes = std::fs::read(cache_path_all_replay(fingerprint)).ok()?;
    let cache: AllReplayCache = bincode::deserialize(&bytes).ok()?;
    (cache.fingerprint == fingerprint).then_some(cache)
}

fn cached_duplicate_clusters(items: Vec<DuplicateCluster>) -> Vec<CachedDuplicateCluster> {
    items
        .into_iter()
        .map(|c| CachedDuplicateCluster {
            avg_similarity: c.avg_similarity,
            chunks: c.chunks.iter().map(CachedCodeChunk::from).collect(),
        })
        .collect()
}

pub(crate) fn store_all_replay_cache(
    fp: &str,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    focus: &FocusFilter,
    full: &FullCheckCache,
) {
    if !opts.bypass_gate || opts.show_timing || opts.suppress_final_status {
        return;
    }
    let (mut violations, py_dups, rs_dups, cache) =
        cached_duplicates(full.clone(), opts.gate_config, focus);
    violations.extend(cached_coverage_viols(&cache, focus));
    let replay = AllReplayCache {
        fingerprint: fp.to_string(),
        py_paths: cache.py_paths.clone(),
        rs_paths: cache.rs_paths.clone(),
        focus_paths: focus.cache_focus_paths(),
        focus_restrict: focus.is_active(),
        py_file_count: cache.py_file_count,
        rs_file_count: cache.rs_file_count,
        code_unit_count: cache.code_unit_count,
        statement_count: cache.statement_count,
        graph_nodes: cache.graph_nodes,
        graph_edges: cache.graph_edges,
        violations: violations.iter().map(CachedViolation::from).collect(),
        py_duplicates: cached_duplicate_clusters(py_dups),
        rs_duplicates: cached_duplicate_clusters(rs_dups),
        file_content_digests: cache.file_content_digests.clone(),
        file_metadata_fingerprints: cache.file_metadata_fingerprints.clone(),
        rslip_fingerprint: cache.rslip_fingerprint.clone(),
        rust_coverage_fingerprint: cache.rust_coverage_fingerprint.clone(),
    };
    let dir = check_cache::cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(bytes) = bincode::serialize(&replay) else {
        return;
    };
    let _ = std::fs::write(cache_path_all_replay(&replay.fingerprint), bytes);
}

fn emit_all_replay(cache: AllReplayCache) -> bool {
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        cache.py_file_count + cache.rs_file_count,
        cache.code_unit_count,
        cache.statement_count,
        cache.graph_nodes,
        cache.graph_edges
    );
    let violations: Vec<_> = cache
        .violations
        .into_iter()
        .map(CachedViolation::into_violation)
        .collect();
    let py_dups: Vec<_> = cache
        .py_duplicates
        .into_iter()
        .map(|c| DuplicateCluster {
            avg_similarity: c.avg_similarity,
            chunks: c
                .chunks
                .into_iter()
                .map(CachedCodeChunk::into_chunk)
                .collect(),
        })
        .collect();
    let rs_dups: Vec<_> = cache
        .rs_duplicates
        .into_iter()
        .map(|c| DuplicateCluster {
            avg_similarity: c.avg_similarity,
            chunks: c
                .chunks
                .into_iter()
                .map(CachedCodeChunk::into_chunk)
                .collect(),
        })
        .collect();
    kiss::cli_output::print_violations(&violations);
    kiss::cli_output::print_duplicates("Python", &py_dups);
    kiss::cli_output::print_duplicates("Rust", &rs_dups);
    let has_violations = !(violations.is_empty() && py_dups.is_empty() && rs_dups.is_empty());
    kiss::cli_output::print_final_status(has_violations);
    !has_violations
}

pub(crate) fn try_run_cached_all_replay(
    opts: &crate::analyze::AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus: &FocusFilter,
    fp: &str,
) -> Option<bool> {
    if !opts.bypass_gate {
        return None;
    }
    let cache = load_all_replay_cache(fp)?;
    if !same_all_replay_paths(py_files, rs_files, focus, &cache) {
        return None;
    }
    if !content_digest::verify_cached_file_state(
        &cache.file_metadata_fingerprints,
        &cache.file_content_digests,
        py_files,
        rs_files,
    ) {
        return None;
    }
    let repo_root = std::path::Path::new(opts.universe);
    if kiss::rslip_bridge::rslip_database_fingerprint(repo_root) != cache.rslip_fingerprint {
        return None;
    }
    if kiss::rust_llvm_cov::backend_fingerprint(repo_root) != cache.rust_coverage_fingerprint {
        return None;
    }
    Some(emit_all_replay(cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiss::check_cache::CachedCodeChunk;

    fn replay_cache(py: &str) -> AllReplayCache {
        AllReplayCache {
            fingerprint: "fp".into(),
            py_paths: vec![py.into()],
            rs_paths: vec![],
            focus_paths: vec![],
            focus_restrict: false,
            py_file_count: 1,
            rs_file_count: 0,
            code_unit_count: 1,
            statement_count: 1,
            graph_nodes: 0,
            graph_edges: 0,
            violations: vec![],
            py_duplicates: vec![],
            rs_duplicates: vec![],
            file_content_digests: vec![],
            file_metadata_fingerprints: vec![],
            rslip_fingerprint: String::new(),
            rust_coverage_fingerprint: String::new(),
        }
    }

    #[test]
    fn same_all_replay_paths_rejects_path_and_focus_mismatches() {
        let py = PathBuf::from("src/a.py");
        let focus = FocusFilter::unrestricted();
        let cache = replay_cache("src/a.py");
        assert!(same_all_replay_paths(
            std::slice::from_ref(&py),
            &[],
            &focus,
            &cache
        ));
        assert!(!same_all_replay_paths(
            &[PathBuf::from("src/b.py")],
            &[],
            &focus,
            &cache
        ));

        let mut focused = replay_cache("src/a.py");
        focused.focus_restrict = true;
        focused.focus_paths = vec!["src/a.py".into()];
        assert!(!same_all_replay_paths(&[py], &[], &focus, &focused));
    }

    #[test]
    fn emit_all_replay_reports_duplicates_and_clean_cache() {
        assert!(emit_all_replay(replay_cache("src/clean.py")));

        let mut cache = replay_cache("src/dup.py");
        cache.py_duplicates.push(CachedDuplicateCluster {
            avg_similarity: 1.0,
            chunks: vec![
                CachedCodeChunk {
                    file: "src/dup.py".into(),
                    name: "a".into(),
                    start_line: 1,
                    end_line: 2,
                    normalized: "x".into(),
                },
                CachedCodeChunk {
                    file: "src/dup.py".into(),
                    name: "b".into(),
                    start_line: 4,
                    end_line: 5,
                    normalized: "x".into(),
                },
            ],
        });
        assert!(
            !emit_all_replay(cache),
            "duplicate clusters should make compact replay fail"
        );
    }

    #[test]
    fn try_run_cached_all_replay_ignores_default_checks() {
        let py_config = kiss::Config::python_defaults();
        let rs_config = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let opts = crate::analyze::AnalyzeOptions {
            universe: ".",
            focus_paths: &[],
            py_config: &py_config,
            rs_config: &rs_config,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &gate,
            ignore_prefixes: &[],
            show_timing: false,
            suppress_final_status: false,
            jobs: None,
            collect_stats: false,
        };

        assert!(
            try_run_cached_all_replay(&opts, &[], &[], &FocusFilter::unrestricted(), "fp")
                .is_none()
        );
    }
}
