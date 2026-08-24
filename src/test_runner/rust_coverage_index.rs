use crate::test_runner::line_selection;
#[cfg(test)]
use kiss::rpytest_runner::TestStatus;
#[cfg(test)]
use kiss::rust_llvm_cov_runner::RustLineCoverage;
use kiss::rust_llvm_cov_runner::{
    CoverageOutputMode, RustCoverageBatchRequest, RustCoverageToolIdentity,
    placeholder_delegated_runner_fields, resolve_batch_request_runners,
};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const CACHE_SCHEMA_VERSION: &str = kiss::rust_llvm_cov_runner::CACHE_SCHEMA_VERSION;
#[cfg(test)]
pub(crate) const LEGACY_INDEX_SCHEMA_VERSION: &str = "rust-llvm-cov-index-v1";

pub(crate) const RUST_COVERAGE_ENV_KEYS: &[&str] = &[
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_TARGET_DIR",
    "LLVM_PROFILE_FILE",
    "KISS_RUST_LLVM_COV_HOLD_BEFORE_GO_MS",
];
pub(crate) fn relevant_rust_batch_env() -> BTreeMap<String, String> {
    kiss::env_map_from_allowlist(RUST_COVERAGE_ENV_KEYS)
}

pub(crate) fn rust_coverage_cache_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("rust_llvm_cov_cache")
}

pub(crate) fn rust_coverage_entry_paths(cache_root: &Path) -> Vec<PathBuf> {
    kiss::json_entry_paths(cache_root)
}

pub(crate) fn create_new_file(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn unique_suffix() -> String {
    kiss::kiss_publication_barrier::unique_process_suffix()
}

#[cfg(test)]
pub(crate) use crate::test_runner::runners::command_stdout;
#[cfg(test)]
pub(crate) use kiss::rust_llvm_cov_runner::is_cargo_config_input_path;
pub(crate) use kiss::rust_llvm_cov_runner::{repo_relative_coverage_file, repo_relative_path};

pub(crate) fn current_rust_coverage_batch_identity(
    repo_root: &Path,
    test_args: &[String],
) -> Result<kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity, String> {
    let (req, tools) = resolved_rust_batch_request_parts(repo_root, test_args)?;
    kiss::rust_llvm_cov_runner::batch_identity(&req, &tools)
        .map_err(|err| format!("batch identity: {err}"))
}

pub(crate) fn current_rust_runner_map_fingerprint(
    repo_root: &Path,
    test_args: &[String],
) -> Result<String, String> {
    let (req, _) = resolved_rust_batch_request_parts(repo_root, test_args)?;
    Ok(req.runner_map_fingerprint)
}

pub(crate) fn resolved_rust_batch_request_parts(
    repo_root: &Path,
    test_args: &[String],
) -> Result<(RustCoverageBatchRequest, RustCoverageToolIdentity), String> {
    let (delegated_runners, runner_map_fingerprint, host_platform) =
        placeholder_delegated_runner_fields();
    let mut req = RustCoverageBatchRequest {
        cwd: repo_root.to_path_buf(),
        source_root: repo_root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: rust_coverage_cache_root(repo_root),
        logical_selectors: Vec::new(),
        cargo_args: vec!["--workspace".to_string()],
        test_args: test_args.to_vec(),
        env: relevant_rust_batch_env(),
        force_rerun: false,
        jobs: 1,
        generated_config: repo_root.join(".kiss/rust_llvm_cov_cache/runs/plan/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners,
        runner_map_fingerprint,
        host_platform,
        coverage_output_mode: CoverageOutputMode::SelectorEntries,
        selector_timeout_millis: BTreeMap::new(),
    };
    resolve_batch_request_runners(&mut req).map_err(|err| format!("{err:?}"))?;
    let tools = tool_identity::cached_rust_coverage_tool_identity(repo_root)?;
    Ok((req, tools))
}

pub(crate) fn publish_rust_derived_state_with_filter(
    repo_root: &Path,
    population_selectors: Option<&[String]>,
    test_args: &[String],
    _is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<(), String> {
    let (mut req, tools) = resolved_rust_batch_request_parts(repo_root, test_args)?;
    let identity = kiss::rust_llvm_cov_runner::batch_identity(&req, &tools)
        .map_err(|err| format!("batch identity: {err}"))?;
    let mut selectors = match population_selectors {
        Some(selectors) => selectors.to_vec(),
        None => kiss::rust_llvm_cov_runner::load_current_population_state(
            &rust_coverage_cache_root(repo_root),
            repo_root,
            &identity,
            None,
        )
        .map(|state| state.selectors)
        .unwrap_or_default(),
    };
    selectors.sort();
    selectors.dedup();
    req.logical_selectors = selectors.clone();
    req.population_publication_selectors = Some(selectors.clone());
    let identity = kiss::rust_llvm_cov_runner::batch_identity(&req, &tools)
        .map_err(|err| format!("batch identity: {err}"))?;
    kiss::rust_llvm_cov_runner::publish_derived_state(&req, &tools, &identity, &selectors, true)
        .map_err(|err| format!("{err:?}"))?;
    Ok(())
}

pub(crate) fn rust_population_manifest_is_current_for_args(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> bool {
    let Ok(identity) = current_rust_coverage_batch_identity(repo_root, test_args) else {
        return false;
    };
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    kiss::rust_llvm_cov_runner::load_current_population_state(
        &rust_coverage_cache_root(repo_root),
        repo_root,
        &identity,
        Some(&expected),
    )
    .is_some()
}

#[allow(dead_code)]
pub(crate) fn load_current_rust_population_state(
    repo_root: &Path,
    selectors: Option<&[String]>,
    test_args: &[String],
) -> Option<kiss::rust_llvm_cov_runner::RustPopulationState> {
    let identity = current_rust_coverage_batch_identity(repo_root, test_args).ok()?;
    kiss::rust_llvm_cov_runner::load_current_population_state(
        &rust_coverage_cache_root(repo_root),
        repo_root,
        &identity,
        selectors,
    )
}

#[path = "rust_coverage_index/tool_identity.rs"]
mod tool_identity;
pub(crate) use tool_identity::rust_coverage_tool_versions_from_cache_or_detect;

#[path = "rust_coverage_index/selection.rs"]
mod selection;
pub(crate) use selection::{
    ResolvedRustPopulation, resolve_rust_population_state, select_rust_source_selectors_for_basis,
};

#[path = "rust_coverage_index/line_select.rs"]
mod line_select;
pub(crate) use line_select::selectors_by_changed_file_line;
#[cfg(test)]
use line_select::{load_entries_for_line_selection, load_entry_for_line_selection};

pub(crate) type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;

#[cfg(test)]
pub(crate) fn build_test_rust_coverage_index(
    repo_root: &Path,
) -> Result<RustCoverageIndex, String> {
    build_rust_coverage_index_with_filter(repo_root, |path, repo_root| {
        repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    })
}

#[cfg(test)]
pub(crate) fn select_rust_source_selectors_from_index(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_args: &[String],
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let index = load_current_rust_coverage_index(repo_root, test_args)?;
    selectors_for_source_paths(repo_root, source_paths, &index)
}

#[cfg(test)]
pub(crate) fn select_rust_source_selectors_for_changed_lines(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Option<BTreeSet<String>> {
    if changed_lines.is_empty() {
        return Some(BTreeSet::new());
    }
    let generation = current_rust_coverage_batch_identity(repo_root, &[])
        .ok()?
        .generation_fingerprint;
    let cache_root = rust_coverage_cache_root(repo_root);
    let entries = load_entries_for_line_selection(&cache_root, &generation);
    if entries.is_empty() {
        return None;
    }
    let mut selectors = BTreeSet::new();
    for (source_path, wanted_lines) in changed_lines {
        if wanted_lines.is_empty() {
            return None;
        }
        let rel = repo_relative_path(repo_root, source_path)?;
        let mut file_selectors = BTreeSet::new();
        for (selector, coverage) in &entries {
            for (file, covered_lines) in &coverage.files {
                if repo_relative_coverage_file(repo_root, file).as_deref() == Some(rel.as_str())
                    && !wanted_lines.is_disjoint(covered_lines)
                {
                    file_selectors.insert(selector.clone());
                    break;
                }
            }
        }
        if file_selectors.is_empty() {
            return None;
        }
        selectors.extend(file_selectors);
    }
    Some(selectors)
}

#[cfg(test)]
pub(crate) fn select_rust_source_selectors_hybrid(
    repo_root: &Path,
    source_paths: &[PathBuf],
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    test_args: &[String],
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let population = load_current_rust_population_state(repo_root, None, test_args)?;
    let index = population.line_index;
    let changed_rels = changed_line_rels(repo_root, changed_lines);
    let line_selectors_by_file = selectors_by_changed_file_line(
        repo_root,
        &changed_rels,
        &population.generation_fingerprint,
    );
    let mut selectors = BTreeSet::new();
    for source_path in source_paths {
        let rel = repo_relative_path(repo_root, source_path)?;
        if let Some(file_selectors) = index.get(&rel).filter(|selectors| !selectors.is_empty()) {
            let selected_for_file = line_selectors_by_file
                .get(&rel)
                .filter(|selectors| !selectors.is_empty())
                .cloned()
                .unwrap_or_else(|| file_selectors.clone());
            selectors.extend(selected_for_file);
        }
    }
    Some(selectors)
}

fn changed_line_rels(
    repo_root: &Path,
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    line_selection::changed_line_rels(repo_root, changed_lines, repo_relative_path)
}

pub(crate) fn selectors_for_source_paths(
    repo_root: &Path,
    source_paths: &[PathBuf],
    index: &RustCoverageIndex,
) -> Option<BTreeSet<String>> {
    let mut selectors = BTreeSet::new();
    for source_path in source_paths {
        let rel = repo_relative_path(repo_root, source_path)?;
        if let Some(file_selectors) = index.get(&rel).filter(|selectors| !selectors.is_empty()) {
            selectors.extend(file_selectors.iter().cloned());
        }
    }
    Some(selectors)
}

#[cfg(test)]
fn build_rust_coverage_index_with_filter(
    repo_root: &Path,
    is_indexable: impl Fn(&Path, &Path) -> bool,
) -> Result<RustCoverageIndex, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let mut files: RustCoverageIndex = BTreeMap::new();
    for entry_path in rust_coverage_entry_paths(&cache_root) {
        let Some((selector, status, coverage)) = load_entry_for_line_selection(&entry_path, "")
        else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for file in coverage.files.keys() {
            let path = Path::new(file);
            if is_indexable(path, repo_root) {
                let rel = repo_relative_coverage_file(repo_root, file)
                    .expect("indexable Rust coverage path has repo-relative form");
                files.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
#[path = "rust_coverage_index/test_support.rs"]
mod test_support;

#[cfg(test)]
pub(crate) use test_support::{
    load_current_rust_coverage_index, normalized_repo_root, rebuild_rust_coverage_index,
    rust_coverage_index_path, rust_population_manifest_path,
    write_rust_population_manifest_for_args, write_test_entry,
};

#[cfg(test)]
#[path = "rust_coverage_index_witness_test.rs"]
mod coverage_witness;

#[cfg(test)]
#[path = "rust_coverage_index_test.rs"]
mod tests;
#[cfg(test)]
#[path = "rust_coverage_index_b_test.rs"]
mod tests_b;

#[cfg(test)]
#[path = "rust_coverage_index_reusable_test.rs"]
mod reusable_tests;

#[cfg(test)]
#[path = "rust_coverage_index_manifest_test.rs"]
mod manifest_tests;
#[cfg(test)]
#[path = "rust_coverage_index_reusable_integration_test.rs"]
mod reusable_integration_tests;
#[cfg(test)]
#[path = "rust_coverage_index_reusable_line_test.rs"]
mod reusable_line_tests;
