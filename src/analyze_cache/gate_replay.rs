use crate::analyze::FocusFilter;
use kiss::check_cache::CachedViolation;
use kiss::{Violation, check_cache};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

use super::{content_digest, fingerprint_for_check};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GateFailureReplayCache {
    fingerprint: String,
    py_paths: Vec<String>,
    rs_paths: Vec<String>,
    focus_paths: Vec<String>,
    focus_restrict: bool,
    violations: Vec<CachedViolation>,
    file_content_digests: Vec<(String, u64)>,
    file_metadata_fingerprints: Vec<(String, u64)>,
    rslip_fingerprint: String,
    rust_coverage_fingerprint: String,
}

fn cache_path_gate_failure(fingerprint: &str) -> PathBuf {
    check_cache::cache_dir().join(format!("check_gate_failure_{fingerprint}.bin"))
}

fn same_gate_replay_paths(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus: &FocusFilter,
    cache: &GateFailureReplayCache,
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

pub(crate) fn store_gate_failure_replay_cache(
    fp: String,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus: &FocusFilter,
    violations: &[Violation],
) {
    if violations.is_empty() || opts.show_timing || opts.suppress_final_status || opts.bypass_gate {
        return;
    }
    let py_paths: Vec<String> = py_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let rs_paths: Vec<String> = rs_files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let py_path_bufs: Vec<PathBuf> = py_paths.iter().map(PathBuf::from).collect();
    let rs_path_bufs: Vec<PathBuf> = rs_paths.iter().map(PathBuf::from).collect();
    let mut file_content_digests = content_digest::content_digests_for_paths(&py_path_bufs);
    file_content_digests.extend(content_digest::content_digests_for_paths(&rs_path_bufs));
    file_content_digests.sort_by(|a, b| a.0.cmp(&b.0));
    let mut file_metadata_fingerprints =
        content_digest::metadata_fingerprints_for_paths(&py_path_bufs);
    file_metadata_fingerprints.extend(content_digest::metadata_fingerprints_for_paths(
        &rs_path_bufs,
    ));
    file_metadata_fingerprints.sort_by(|a, b| a.0.cmp(&b.0));
    let repo_root = std::path::Path::new(opts.universe);
    let cache = GateFailureReplayCache {
        fingerprint: fp,
        py_paths,
        rs_paths,
        focus_paths: focus.cache_focus_paths(),
        focus_restrict: focus.is_active(),
        violations: violations.iter().map(CachedViolation::from).collect(),
        file_content_digests,
        file_metadata_fingerprints,
        rslip_fingerprint: kiss::rslip_bridge::rslip_database_fingerprint(repo_root),
        rust_coverage_fingerprint: kiss::rust_llvm_cov::backend_fingerprint(repo_root),
    };
    let dir = check_cache::cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(bytes) = bincode::serialize(&cache) else {
        return;
    };
    let _ = std::fs::write(cache_path_gate_failure(&cache.fingerprint), bytes);
}

fn load_gate_failure_replay_cache(fingerprint: &str) -> Option<GateFailureReplayCache> {
    let bytes = std::fs::read(cache_path_gate_failure(fingerprint)).ok()?;
    let cache: GateFailureReplayCache = bincode::deserialize(&bytes).ok()?;
    (cache.fingerprint == fingerprint).then_some(cache)
}

fn print_gate_failure_replay_violations(viols: Vec<Violation>) {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    for v in viols {
        if v.suggestion.is_empty() {
            let _ = writeln!(
                w,
                "VIOLATION:{}:{}:{}:{}: {}",
                v.metric,
                v.file.display(),
                v.line,
                v.unit_name,
                v.message
            );
        } else {
            let _ = writeln!(
                w,
                "VIOLATION:{}:{}:{}:{}: {} {}",
                v.metric,
                v.file.display(),
                v.line,
                v.unit_name,
                v.message,
                v.suggestion
            );
        }
    }
}

pub(crate) fn try_run_cached_gate_failure(
    opts: &crate::analyze::AnalyzeOptions<'_>,
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    focus: &FocusFilter,
) -> Option<bool> {
    if opts.bypass_gate {
        return None;
    }
    let fp = fingerprint_for_check(
        py_files,
        rs_files,
        opts.py_config,
        opts.rs_config,
        opts.gate_config,
    );
    let cache = load_gate_failure_replay_cache(&fp)?;
    if !same_gate_replay_paths(py_files, rs_files, focus, &cache) {
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
    let viols: Vec<_> = cache
        .violations
        .into_iter()
        .map(CachedViolation::into_violation)
        .collect();
    print_gate_failure_replay_violations(viols);
    Some(false)
}
