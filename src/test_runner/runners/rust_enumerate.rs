use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kiss::rust_llvm_cov_runner::repo_relative_path;
use rayon::prelude::*;

use crate::test_runner::lang_rust::workspace::{
    cargo_workspace_member_manifest_dirs, is_workspace_rust_selector_file_cached,
};
use crate::test_runner::targets::rust_direct_test_selectors;

#[path = "rust_enumerate_dynamic.rs"]
mod rust_enumerate_dynamic;
use rust_enumerate_dynamic::rust_file_needs_dynamic_listing;

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
    source.contains("#[test]") || source.contains("#[ test]") || source.contains("cfg_attr")
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
            let mut files: Vec<PathBuf> = rs_files
                .into_iter()
                .filter(|path| {
                    is_workspace_rust_selector_file_cached(
                        path,
                        &member_manifest_dirs,
                        &mut nearest_cache,
                    )
                })
                .collect();
            retain_cargo_reachable_rust_files(repo_root, &mut files);
            files
        }
        Err(_) => rs_files,
    }
}

fn retain_cargo_reachable_rust_files(repo_root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(reachable) = kiss::code_roles::reachable_workspace_rust_sources(repo_root) else {
        return;
    };
    if reachable.is_empty() {
        return;
    }
    files.retain(|path| reachable.contains(&kiss::rust_include::canonical_path(path)));
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

fn dynamic_rust_selectors(
    repo_root: &Path,
    ignore: &[String],
    candidate_sources: &[PathBuf],
) -> Result<Vec<(PathBuf, String)>, String> {
    let (mut request, tools) =
        crate::test_runner::rust_coverage_index::resolved_rust_batch_request_parts(repo_root, &[])?;
    request.jobs = rust_dynamic_listing_jobs(repo_root)?;
    let target_sources = kiss::rust_llvm_cov_runner::workspace_test_target_sources(
        &request.cwd,
        &request.cargo,
        &request.cargo_args,
    )
    .map_err(|err| {
        format!("error: kiss test: failed to resolve generated Rust test targets: {err:?}")
    })?;
    request.population_publication_selectors = Some(Vec::new());
    let identity = kiss::rust_llvm_cov_runner::batch_identity(&request, &tools)
        .map_err(|err| format!("error: kiss test: failed to list generated Rust tests: {err}"))?;
    let plan = kiss::rust_llvm_cov_runner::build_rust_coverage_batch_plan(&request)
        .map_err(|err| format!("error: kiss test: failed to list generated Rust tests: {err}"))?;
    let (_, listed_tests) =
        kiss::rust_llvm_cov_runner::build_rust_test_executable_index_with_tests(
            &request, &tools, &identity, &plan,
        )
        .map_err(|err| format!("error: kiss test: failed to list generated Rust tests: {err:?}"))?;
    let mut selectors = BTreeSet::new();
    for listed in listed_tests {
        let source = source_for_listed_test(
            Path::new(&listed.executable),
            &listed.logical_name,
            candidate_sources,
            &target_sources,
        );
        let rel = source
            .strip_prefix(repo_root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if !kiss::path_ignored_by_prefixes(&rel, ignore) {
            selectors.insert((source, listed.logical_name));
        }
    }
    Ok(selectors.into_iter().collect())
}

fn rust_dynamic_listing_jobs(repo_root: &Path) -> Result<usize, String> {
    kiss::TestSectionConfig::try_load_path_only(&kiss::kissconfig_path_for_repo(repo_root))
        .map(|config| config.num_jobs)
        .map_err(|err| format!("error: kiss test: failed to load test configuration: {err}"))
}

fn source_for_listed_test(
    executable: &Path,
    selector: &str,
    candidate_sources: &[PathBuf],
    target_sources: &[(String, PathBuf)],
) -> PathBuf {
    let executable_stem = executable
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .rsplit_once('-')
        .map_or_else(
            || {
                executable
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
            },
            |(stem, _)| stem,
        );
    let matching_targets = target_sources
        .iter()
        .filter(|(name, _)| name == executable_stem)
        .map(|(_, source)| source);
    if let Some(source) = matching_targets.clone().find(|source| {
        !matches!(
            source.file_name().and_then(|value| value.to_str()),
            Some("lib.rs" | "main.rs")
        )
    }) {
        return source.clone();
    }
    matching_targets
        .chain(candidate_sources.iter().filter(|source| {
            source
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| matches!(name, "lib.rs" | "main.rs"))
        }))
        .map(|root| defining_source_for_selector(root, selector, candidate_sources))
        .max_by_key(|source| {
            (
                !matches!(
                    source.file_name().and_then(|value| value.to_str()),
                    Some("lib.rs" | "main.rs")
                ),
                source.components().count(),
            )
        })
        .or_else(|| candidate_sources.first().cloned())
        .unwrap_or_default()
}

fn defining_source_for_selector(
    target_source: &Path,
    selector: &str,
    candidate_sources: &[PathBuf],
) -> PathBuf {
    let Some(filename) = target_source.file_name().and_then(|name| name.to_str()) else {
        return target_source.to_path_buf();
    };
    if !matches!(filename, "lib.rs" | "main.rs") {
        return target_source.to_path_buf();
    }
    let Some(source_root) = target_source.parent() else {
        return target_source.to_path_buf();
    };
    candidate_sources
        .iter()
        .filter_map(|candidate| {
            let relative = candidate.strip_prefix(source_root).ok()?;
            if relative == Path::new(filename) {
                return None;
            }
            let mut parts = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let last = parts.last_mut()?;
            *last = last.strip_suffix(".rs")?.to_string();
            if last == "mod" {
                parts.pop();
            }
            let module = parts.join("::");
            (!module.is_empty()
                && (selector == module || selector.starts_with(&format!("{module}::"))))
            .then_some((module.len(), candidate.clone()))
        })
        .max_by_key(|(prefix_len, _)| *prefix_len)
        .map_or_else(|| target_source.to_path_buf(), |(_, path)| path)
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
    let needs_dynamic_listing = rs_files
        .iter()
        .any(|path| rust_file_needs_dynamic_listing(path));
    let mut entries = entries;
    if needs_dynamic_listing {
        #[cfg(test)]
        let testing_current_crate = repo_root == Path::new(env!("CARGO_MANIFEST_DIR"));
        #[cfg(not(test))]
        let testing_current_crate = false;
        if !testing_current_crate {
            let known: BTreeSet<_> = entries
                .iter()
                .map(|(_, selector)| selector.clone())
                .collect();
            entries.extend(
                dynamic_rust_selectors(repo_root, ignore, &rs_files)?
                    .into_iter()
                    .filter(|(_, selector)| !known.contains(selector)),
            );
        }
    }
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

#[cfg(test)]
#[path = "rust_enumerate_test.rs"]
mod tests;
