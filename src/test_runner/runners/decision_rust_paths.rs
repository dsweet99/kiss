use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::rust_coverage_index::ResolvedRustPopulation;

use super::{ChangedTestSelectors, changed_lines_for_sources, changed_test_selectors_by_language};

pub(super) struct PreparedRustInputs {
    pub(super) py_source_paths: Vec<PathBuf>,
    pub(super) rust_source_paths: Vec<PathBuf>,
    pub(super) python_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(super) rust_changed_lines: BTreeMap<PathBuf, BTreeSet<u32>>,
    pub(super) changed_tests: ChangedTestSelectors,
    pub(super) rust_resolved: Option<ResolvedRustPopulation>,
    pub(super) rust_vcs_source_paths: usize,
    pub(super) rust_snapshot_delta_modified: usize,
    pub(super) rust_snapshot_delta_structural: bool,
}

pub(super) fn prepare_rust_inputs(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_paths: &[PathBuf],
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_test_args: &[String],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
) -> Result<PreparedRustInputs, String> {
    let plan_trace = std::env::var_os("KISS_PLAN_TRACE").is_some();
    let mut mark = std::time::Instant::now();
    let mut lap = |label: &str| {
        if plan_trace {
            eprintln!(
                "KISS_PLAN_TRACE prep_{label}_ms={}",
                mark.elapsed().as_millis()
            );
            mark = std::time::Instant::now();
        }
    };
    let (py_source_paths, rust_vcs_source_paths) = super::split_source_paths(source_paths);
    let rust_vcs_source_count = rust_vcs_source_paths.len();
    let (py_test_paths, rust_vcs_test_paths) = split_test_paths(test_paths);
    lap("split");
    let rust_vcs_changed_lines = changed_lines_for_sources(changed_lines, &rust_vcs_source_paths);
    let rust_resolution = resolve_effective_rust_paths(
        repo_root,
        &rust_vcs_source_paths,
        &rust_vcs_test_paths,
        rust_test_args,
        lang_filter,
        ignore,
        &rust_vcs_changed_lines,
    )?;
    lap("resolve_rust");
    let rust_changed_lines = effective_rust_changed_lines(
        changed_lines,
        &rust_resolution.source_paths,
        rust_resolution.resolved.as_ref(),
    );
    lap("rust_changed_lines");

    let mut effective_test_paths = py_test_paths;
    let line_precise = !rust_changed_lines.is_empty() && rust_changed_lines.len() <= 1;
    if !line_precise
        && rust_vcs_test_paths.is_empty()
        && check_aggregate_covers_changed_rust_sources(
            repo_root,
            &rust_resolution.source_paths,
            rust_resolution.resolved.as_ref(),
        )
    {
    } else {
        effective_test_paths.extend(rust_resolution.test_paths.iter().cloned());
    }
    let changed_tests =
        changed_test_selectors_by_language(repo_root, &effective_test_paths, ignore)?;
    lap("changed_tests");
    Ok(PreparedRustInputs {
        python_changed_lines: changed_lines_for_sources(changed_lines, &py_source_paths),
        py_source_paths,
        rust_source_paths: rust_resolution.source_paths,
        rust_changed_lines,
        changed_tests,
        rust_resolved: rust_resolution.resolved,
        rust_vcs_source_paths: rust_vcs_source_count,
        rust_snapshot_delta_modified: rust_resolution.snapshot_delta_modified,
        rust_snapshot_delta_structural: rust_resolution.snapshot_delta_structural,
    })
}

fn check_aggregate_covers_changed_rust_sources(
    repo_root: &Path,
    rust_source_paths: &[PathBuf],
    resolved: Option<&ResolvedRustPopulation>,
) -> bool {
    let Some(ResolvedRustPopulation::Current { state }) = resolved else {
        return false;
    };
    if !kiss::rust_llvm_cov_runner::is_check_aggregate_population(state) {
        return false;
    }
    for source_path in rust_source_paths {
        let Some(key) =
            crate::test_runner::rust_coverage_index::repo_relative_path(repo_root, source_path)
        else {
            continue;
        };
        if state.line_index.contains_key(&key) {
            return true;
        }
    }
    false
}

fn split_test_paths(test_paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    test_paths
        .iter()
        .cloned()
        .partition(|path| !kiss::Language::is_rust_path(path))
}

struct EffectiveRustPaths {
    source_paths: Vec<PathBuf>,
    test_paths: Vec<PathBuf>,
    resolved: Option<ResolvedRustPopulation>,
    snapshot_delta_modified: usize,
    snapshot_delta_structural: bool,
}

fn resolve_effective_rust_paths(
    repo_root: &Path,
    rust_vcs_source_paths: &[PathBuf],
    rust_vcs_test_paths: &[PathBuf],
    rust_test_args: &[String],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
    rust_vcs_changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
) -> Result<EffectiveRustPaths, String> {
    let needs_resolution = lang_filter != Some(kiss::Language::Python)
        && (!rust_vcs_source_paths.is_empty() || !rust_vcs_test_paths.is_empty());
    if !needs_resolution {
        return Ok(EffectiveRustPaths {
            source_paths: rust_vcs_source_paths.to_vec(),
            test_paths: rust_vcs_test_paths.to_vec(),
            resolved: None,
            snapshot_delta_modified: 0,
            snapshot_delta_structural: false,
        });
    }
    let resolved = crate::test_runner::rust_coverage_index::resolve_rust_population_state(
        crate::test_runner::rust_coverage_index::ResolveRustPopulationArgs {
            repo_root,
            ignore,
            rust_source_paths: rust_vcs_source_paths,
            rust_changed_lines: rust_vcs_changed_lines,
            expected_selectors: None,
            test_args: rust_test_args,
        },
    )?;
    let (source_paths, test_paths, modified, structural) = effective_paths_for_resolution(
        &resolved,
        rust_vcs_source_paths,
        rust_vcs_test_paths,
        repo_root,
        ignore,
    )?;
    Ok(EffectiveRustPaths {
        source_paths,
        test_paths,
        resolved: Some(resolved),
        snapshot_delta_modified: modified,
        snapshot_delta_structural: structural,
    })
}

fn effective_paths_for_resolution(
    resolved: &ResolvedRustPopulation,
    rust_vcs_source_paths: &[PathBuf],
    rust_vcs_test_paths: &[PathBuf],
    _repo_root: &Path,
    _ignore: &[String],
) -> Result<(Vec<PathBuf>, Vec<PathBuf>, usize, bool), String> {
    match resolved {
        ResolvedRustPopulation::Current { .. } => Ok((
            rust_vcs_source_paths.to_vec(),
            rust_vcs_test_paths.to_vec(),
            0,
            false,
        )),
        ResolvedRustPopulation::ReusablePrior { delta, .. } => match delta {
            kiss::rust_llvm_cov_runner::RustSnapshotDelta::Modified(paths) => {
                let roles = roles_for_modified_paths(paths)?;
                let (source_paths, test_paths) =
                    crate::test_runner::runners::partition_changed_paths_with_roles(paths, &roles);
                Ok((source_paths, test_paths, paths.len(), false))
            }
            kiss::rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange => {
                Ok((Vec::new(), Vec::new(), 0, true))
            }
            kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged => {
                Ok((Vec::new(), Vec::new(), 0, false))
            }
        },
        ResolvedRustPopulation::StructuralStale => Ok((Vec::new(), Vec::new(), 0, true)),
        ResolvedRustPopulation::ColdStale => Ok((
            rust_vcs_source_paths.to_vec(),
            rust_vcs_test_paths.to_vec(),
            0,
            false,
        )),
    }
}

fn roles_for_modified_paths(
    paths: &[PathBuf],
) -> Result<kiss::code_roles::SourceRoleIndex, String> {
    let existing: Vec<_> = paths.iter().filter(|path| path.exists()).cloned().collect();
    crate::test_runner::runners::roles_for_changed_paths(&existing).map_err(|err| err.to_string())
}

fn effective_rust_changed_lines(
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_source_paths: &[PathBuf],
    resolved: Option<&ResolvedRustPopulation>,
) -> BTreeMap<PathBuf, BTreeSet<u32>> {
    if let Some(resolved) = resolved
        && matches!(
            resolved,
            ResolvedRustPopulation::Current { .. } | ResolvedRustPopulation::ReusablePrior { .. }
        )
    {
        return changed_lines_for_sources(changed_lines, rust_source_paths);
    }
    BTreeMap::new()
}

#[cfg(test)]
#[path = "decision_rust_paths_test.rs"]
mod tests;
