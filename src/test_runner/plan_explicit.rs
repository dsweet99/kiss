
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use kiss::Language;

use super::super::runners;
use super::super::targets::resolve_target_operands;
use super::{PlannedSelectors, planned_from_selector_plan};

pub(super) fn plan_explicit_target_selectors(
    repo_root: &std::path::Path,
    targets: &[String],
    ignore: &[String],
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
) -> Result<PlannedSelectors, String> {
    let query = resolve_target_operands(repo_root, targets, lang_filter, ignore, extras.python)
        .map_err(|e| format!("error: kiss test: {e}"))?;
    let mut source_paths = Vec::new();
    source_paths.extend(query.python_files.iter().cloned());
    source_paths.extend(query.rust_files.iter().cloned());
    source_paths.extend(query.python_lines.keys().cloned());
    source_paths.extend(query.rust_lines.keys().cloned());
    source_paths.sort();
    source_paths.dedup();
    reject_non_member_rust_targets(repo_root, &query)?;
    let mut changed_lines: BTreeMap<PathBuf, BTreeSet<u32>> = BTreeMap::new();
    for (path, lines) in query.python_lines.iter().chain(query.rust_lines.iter()) {
        changed_lines
            .entry(path.clone())
            .or_default()
            .extend(lines.iter().copied());
    }
    let direct_python: Vec<_> = query.direct_python.into_iter().collect();
    let direct_rust: Vec<_> = query.direct_rust.into_iter().collect();


    if source_paths.is_empty() {
        return Ok(PlannedSelectors {
            repo_root: repo_root.to_path_buf(),
            sel: crate::test_runner::language_keyed::LanguageKeyed {
                python: direct_python,
                rust: direct_rust,
            },
            population_required: crate::test_runner::language_keyed::LanguageKeyed {
                python: false,
                rust: false,
            },
            source_paths: crate::test_runner::language_keyed::LanguageKeyed {
                python: Vec::new(),
                rust: Vec::new(),
            },
            vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
                python: 0,
                rust: 0,
            },
            snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
                python: 0,
                rust: 0,
            },
            snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
                python: false,
                rust: false,
            },
            prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
                python: Vec::new(),
                rust: Vec::new(),
            },
            coverage_decision_engine_used: false,
            selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
                python: crate::test_runner::coverage_decision::SelectionBasis::Current,
                rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
            },
            ignore: ignore.to_vec(),
            workspace_files_fingerprint: None,
            skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
                python: true,
                rust: false,
            },
        });
    }
    let input = runners::CombinedSelectorInput {
        repo_root,
        source_paths: &source_paths,
        test_paths: &[],
        changed_lines: &changed_lines,
        test_args: extras,
        lang_filter,
        ignore,
        extra_direct_python: &direct_python,
        extra_direct_rust: &direct_rust,
        include_prior_failures: false,
    };
    let selector_plan = runners::combined_selectors_with_direct(input)?;
    Ok(planned_from_selector_plan(
        repo_root.to_path_buf(),
        selector_plan,
        ignore.to_vec(),
    ))
}

fn reject_non_member_rust_targets(
    repo_root: &std::path::Path,
    query: &crate::test_runner::targets::TargetSelectionQuery,
) -> Result<(), String> {
    let mut rust_paths: Vec<PathBuf> = query.rust_files.iter().cloned().collect();
    rust_paths.extend(query.rust_lines.keys().cloned());

    for selector in &query.direct_rust {
        let path_part = selector.split_once("::").map_or(selector.as_str(), |(p, _)| p);
        let candidate = PathBuf::from(path_part);
        if candidate.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs")) {
            let abs = if candidate.is_absolute() {
                candidate
            } else {
                repo_root.join(candidate)
            };
            rust_paths.push(abs);
        }
    }
    rust_paths.sort();
    rust_paths.dedup();
    let roots = crate::test_runner::lang_rust::workspace::non_member_rust_crate_roots(
        repo_root, &rust_paths,
    )?;
    if roots.is_empty() {
        return Ok(());
    }
    Err(format!(
        "error: kiss test: nested Cargo crate(s) are not root workspace members (coverage unsupported): {}",
        roots.join(", ")
    ))
}
