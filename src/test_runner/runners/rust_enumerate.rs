use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kiss::rust_llvm_cov_runner::repo_relative_path;
use rayon::prelude::*;

use crate::test_runner::lang_rust::workspace::{
    cargo_workspace_member_manifest_dirs, is_workspace_rust_selector_file_cached,
};
use crate::test_runner::targets::rust_direct_test_selectors;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParseErrorPolicy {
    Fail,
    Skip,
}

pub fn enumerate_workspace_rust_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Result<Vec<String>, String> {
    let mut selectors = BTreeSet::new();
    for (_path, selector) in
        enumerate_workspace_rust_test_entries(repo_root, ignore, ParseErrorPolicy::Fail)?
    {
        selectors.insert(selector);
    }
    Ok(selectors.into_iter().collect())
}

pub(crate) fn rust_logical_to_kiss_test_ids(
    repo_root: &Path,
    ignore: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();

    for (path, logical) in
        enumerate_workspace_rust_test_entries(repo_root, ignore, ParseErrorPolicy::Skip)?
    {
        let Some(rel) = repo_relative_path(repo_root, &path) else {
            continue;
        };
        let bare = logical
            .rsplit_once("::")
            .map_or(logical.as_str(), |(_, name)| name)
            .to_string();
        map.insert(logical, format!("{rel}::{bare}"));
    }
    Ok(map)
}

fn source_may_contain_rust_test_attr(source: &str) -> bool {
    source.contains("#[test]") || source.contains("#[ test]")
}

fn rust_selector_parse_threads() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);

    match std::env::var("MALLOC_ARENA_MAX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        Some(arenas) if arenas > 0 => arenas.clamp(1, cpus),
        _ => cpus,
    }
}

fn rust_selector_parse_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(rust_selector_parse_threads())
            .build()
            .unwrap_or_else(|_| {
                rayon::ThreadPoolBuilder::new()
                    .build()
                    .expect("rayon ThreadPoolBuilder")
            })
    })
}

fn gather_member_rust_files(repo_root: &Path, ignore: &[String]) -> Vec<PathBuf> {
    let root = repo_root.to_string_lossy().to_string();

    let (_py_files, rs_files) =
        kiss::gather_files_by_lang_opts(&[root], Some(kiss::Language::Rust), ignore, false);
    match cargo_workspace_member_manifest_dirs(repo_root) {
        Ok(member_manifest_dirs) => {
            let mut nearest_cache = HashMap::new();
            rs_files
                .into_iter()
                .filter(|path| {
                    is_workspace_rust_selector_file_cached(
                        path,
                        &member_manifest_dirs,
                        &mut nearest_cache,
                    )
                })
                .collect()
        }
        Err(_) => rs_files,
    }
}

fn selectors_in_rust_file(path: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "error: kiss test: failed to read Rust workspace file {}: {e}",
            path.display()
        )
    })?;
    if !source_may_contain_rust_test_attr(&source) {
        return Ok((path.to_path_buf(), Vec::new()));
    }
    let selectors = rust_direct_test_selectors(path).map_err(|e| {
        format!(
            "error: kiss test: failed to parse Rust workspace file {}: {e}",
            path.display()
        )
    })?;
    Ok((path.to_path_buf(), selectors))
}

fn flatten_parsed_entries(
    parsed: Vec<Result<(PathBuf, Vec<String>), String>>,
    parse_errors: ParseErrorPolicy,
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut entries = Vec::new();
    for result in parsed {
        let (path, selectors) = match result {
            Ok(v) => v,
            Err(e) => {
                if parse_errors == ParseErrorPolicy::Skip {
                    continue;
                }
                return Err(e);
            }
        };
        for selector in selectors {
            entries.push((path.clone(), selector));
        }
    }
    Ok(entries)
}

fn enumerate_workspace_rust_test_entries(
    repo_root: &Path,
    ignore: &[String],
    parse_errors: ParseErrorPolicy,
) -> Result<Vec<(PathBuf, String)>, String> {
    let profile = std::env::var_os("KISS_PROFILE_RUST_PLAN").is_some();
    let t0 = std::time::Instant::now();
    let rs_files = gather_member_rust_files(repo_root, ignore);
    let t_gather = t0.elapsed();
    let n_files = rs_files.len();

    let t_parse_started = std::time::Instant::now();
    let parse_threads = rust_selector_parse_threads();
    let parsed: Vec<Result<(PathBuf, Vec<String>), String>> =
        rust_selector_parse_pool().install(|| {
            rs_files
                .par_iter()
                .map(|p| selectors_in_rust_file(p))
                .collect()
        });
    let t_parse = t_parse_started.elapsed();
    let entries = flatten_parsed_entries(parsed, parse_errors)?;
    if profile {
        eprintln!(
            "KISS_PROFILE_RUST_PLAN gather_ms={} parse_ms={} parse_threads={} files={} selectors={} total_ms={}",
            t_gather.as_millis(),
            t_parse.as_millis(),
            parse_threads,
            n_files,
            entries.len(),
            t0.elapsed().as_millis()
        );
    }
    Ok(entries)
}
