use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_runner::coverage_decision::RustSelectionBasis;
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
            eprintln!("KISS_PLAN_TRACE prep_{label}_ms={}", mark.elapsed().as_millis());
            mark = std::time::Instant::now();
        }
    };
    let (py_source_paths, rust_vcs_source_paths) = super::split_source_paths(source_paths);
    let rust_vcs_source_count = rust_vcs_source_paths.len();
    let (py_test_paths, rust_vcs_test_paths) = split_test_paths(test_paths);
    lap("split");
    let rust_resolution = resolve_effective_rust_paths(
        repo_root,
        &rust_vcs_source_paths,
        &rust_vcs_test_paths,
        rust_test_args,
        lang_filter,
        ignore,
    )?;
    lap("resolve_rust");
    let rust_changed_lines = effective_rust_changed_lines(
        changed_lines,
        &rust_resolution.source_paths,
        rust_resolution.resolved.as_ref(),
    );
    lap("rust_changed_lines");
    // Full check-aggregate file-level select makes Rust changed-test parsing
    // redundant. Keep it when line-precise narrowing may drop to a subset.
    let mut effective_test_paths = py_test_paths;
    let line_precise = !rust_changed_lines.is_empty() && rust_changed_lines.len() <= 1;
    if !line_precise
        && check_aggregate_covers_changed_rust_sources(
            repo_root,
            &rust_resolution.source_paths,
            rust_resolution.resolved.as_ref(),
        )
    {
        // Python changed tests only.
    } else {
        effective_test_paths.extend(rust_resolution.test_paths.iter().cloned());
    }
    let changed_tests = changed_test_selectors_by_language(repo_root, &effective_test_paths)?;
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
    let Some(resolved) = resolved else {
        return false;
    };
    if resolved.basis != RustSelectionBasis::Current {
        return false;
    }
    let Some(state) = resolved.state.as_ref() else {
        return false;
    };
    if !rust_llvm_cov_runner::is_check_aggregate_population(state) {
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
        repo_root,
        ignore,
        rust_vcs_source_paths,
        rust_test_args,
    )?;
    let (source_paths, test_paths, modified, structural) =
        effective_paths_for_resolution(&resolved, rust_vcs_source_paths, rust_vcs_test_paths);
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
) -> (Vec<PathBuf>, Vec<PathBuf>, usize, bool) {
    match resolved.basis {
        RustSelectionBasis::Current => (
            rust_vcs_source_paths.to_vec(),
            rust_vcs_test_paths.to_vec(),
            0,
            false,
        ),
        RustSelectionBasis::ReusablePrior => match resolved.snapshot_delta.as_ref() {
            Some(rust_llvm_cov_runner::RustSnapshotDelta::Modified(paths)) => {
                let (source_paths, test_paths) =
                    crate::test_runner::runners::partition_changed_paths(paths);
                (source_paths, test_paths, paths.len(), false)
            }
            Some(rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange) => {
                (Vec::new(), Vec::new(), 0, true)
            }
            Some(rust_llvm_cov_runner::RustSnapshotDelta::Unchanged) | None => {
                (Vec::new(), Vec::new(), 0, false)
            }
        },
        RustSelectionBasis::Population => {
            let structural = resolved.snapshot_delta.as_ref().is_some_and(|delta| {
                *delta == rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange
            });
            if structural {
                (Vec::new(), Vec::new(), 0, true)
            } else {
                (
                    rust_vcs_source_paths.to_vec(),
                    rust_vcs_test_paths.to_vec(),
                    0,
                    false,
                )
            }
        }
    }
}

fn effective_rust_changed_lines(
    changed_lines: &BTreeMap<PathBuf, BTreeSet<u32>>,
    rust_source_paths: &[PathBuf],
    resolved: Option<&ResolvedRustPopulation>,
) -> BTreeMap<PathBuf, BTreeSet<u32>> {
    if let Some(resolved) = resolved
        && matches!(
            resolved.basis,
            RustSelectionBasis::Current | RustSelectionBasis::ReusablePrior
        )
    {
        return changed_lines_for_sources(changed_lines, rust_source_paths);
    }
    BTreeMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::coverage_decision::CoverageFreshness;

    fn resolved(
        basis: RustSelectionBasis,
        snapshot_delta: Option<rust_llvm_cov_runner::RustSnapshotDelta>,
    ) -> ResolvedRustPopulation {
        ResolvedRustPopulation {
            freshness: match basis {
                RustSelectionBasis::Current => CoverageFreshness::Fresh,
                RustSelectionBasis::ReusablePrior => CoverageFreshness::ReusablePrior,
                RustSelectionBasis::Population => CoverageFreshness::Stale,
            },
            basis,
            state: None,
            snapshot_delta,
        }
    }

    #[test]
    fn prepare_rust_inputs_carries_no_resolution_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let py = tmp.path().join("app.py");
        let rs = tmp.path().join("src").join("lib.rs");
        std::fs::create_dir_all(rs.parent().unwrap()).unwrap();
        std::fs::write(&py, "VALUE = 1\n").unwrap();
        std::fs::write(&rs, "pub fn value() -> u32 { 1 }\n").unwrap();
        let prepared = prepare_rust_inputs(
            tmp.path(),
            &[py.clone(), rs.clone()],
            &[],
            &BTreeMap::from([(py.clone(), BTreeSet::from([1]))]),
            &[],
            Some(kiss::Language::Python),
            &[],
        )
        .unwrap();

        assert_eq!(prepared.py_source_paths, vec![py]);
        assert_eq!(prepared.rust_source_paths, vec![rs]);
        assert_eq!(prepared.rust_vcs_source_paths, 1);
        assert_eq!(prepared.rust_snapshot_delta_modified, 0);
        assert!(!prepared.rust_snapshot_delta_structural);
        assert!(prepared.rust_resolved.is_none());
        assert!(prepared.changed_tests.python.is_empty());
        assert!(prepared.changed_tests.rust.is_empty());
        assert!(prepared.rust_changed_lines.is_empty());
    }

    #[test]
    fn effective_paths_follow_current_and_reusable_delta_rules() {
        let vcs_source = PathBuf::from("src/lib.rs");
        let vcs_test = PathBuf::from("tests/integration.rs");
        assert_eq!(
            effective_paths_for_resolution(
                &resolved(RustSelectionBasis::Current, None),
                std::slice::from_ref(&vcs_source),
                std::slice::from_ref(&vcs_test),
            ),
            (vec![vcs_source], vec![vcs_test], 0, false)
        );

        let modified = PathBuf::from("src/changed.rs");
        assert_eq!(
            effective_paths_for_resolution(
                &resolved(
                    RustSelectionBasis::ReusablePrior,
                    Some(rust_llvm_cov_runner::RustSnapshotDelta::Modified(vec![
                        modified.clone()
                    ])),
                ),
                &[],
                &[],
            ),
            (vec![modified], Vec::new(), 1, false)
        );

        assert_eq!(
            effective_paths_for_resolution(
                &resolved(
                    RustSelectionBasis::ReusablePrior,
                    Some(rust_llvm_cov_runner::RustSnapshotDelta::Unchanged),
                ),
                &[],
                &[],
            ),
            (Vec::new(), Vec::new(), 0, false)
        );
    }

    #[test]
    fn effective_paths_mark_structural_population() {
        assert_eq!(
            effective_paths_for_resolution(
                &resolved(
                    RustSelectionBasis::Population,
                    Some(rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange),
                ),
                &[],
                &[],
            ),
            (Vec::new(), Vec::new(), 0, true)
        );
        let vcs_source = PathBuf::from("src/lib.rs");
        assert_eq!(
            effective_paths_for_resolution(
                &resolved(RustSelectionBasis::Population, None),
                std::slice::from_ref(&vcs_source),
                &[],
            ),
            (vec![vcs_source], Vec::new(), 0, false)
        );
    }

    #[test]
    fn effective_rust_changed_lines_survive_line_aware_basis() {
        let path = PathBuf::from("src/lib.rs");
        let changed = BTreeMap::from([(path.clone(), BTreeSet::from([7]))]);
        assert_eq!(
            effective_rust_changed_lines(
                &changed,
                std::slice::from_ref(&path),
                Some(&resolved(RustSelectionBasis::Current, None)),
            ),
            changed
        );
        assert_eq!(
            effective_rust_changed_lines(
                &changed,
                std::slice::from_ref(&path),
                Some(&resolved(
                    RustSelectionBasis::ReusablePrior,
                    Some(rust_llvm_cov_runner::RustSnapshotDelta::Modified(vec![
                        path.clone()
                    ])),
                )),
            ),
            changed
        );
        assert!(
            effective_rust_changed_lines(
                &changed,
                &[],
                Some(&resolved(
                    RustSelectionBasis::ReusablePrior,
                    Some(rust_llvm_cov_runner::RustSnapshotDelta::Unchanged),
                )),
            )
            .is_empty()
        );
    }

    #[test]
    fn prepared_and_effective_rust_input_unit_witnesses() {
        let prepared: PreparedRustInputs = PreparedRustInputs {
            py_source_paths: Vec::new(),
            rust_source_paths: Vec::new(),
            python_changed_lines: BTreeMap::new(),
            rust_changed_lines: BTreeMap::new(),
            changed_tests: ChangedTestSelectors::default(),
            rust_resolved: None,
            rust_vcs_source_paths: 0,
            rust_snapshot_delta_modified: 0,
            rust_snapshot_delta_structural: false,
        };
        let effective: EffectiveRustPaths = EffectiveRustPaths {
            source_paths: Vec::new(),
            test_paths: Vec::new(),
            resolved: None,
            snapshot_delta_modified: 0,
            snapshot_delta_structural: false,
        };
        assert_eq!(
            prepared.rust_vcs_source_paths,
            effective.snapshot_delta_modified
        );
        assert!(effective.source_paths.is_empty());
        assert!(effective.test_paths.is_empty());
        assert!(effective.resolved.is_none());
        assert!(!effective.snapshot_delta_structural);
    }
}
