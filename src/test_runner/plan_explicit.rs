use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use kiss::Language;

use super::super::lang_registry::rules_for;
use super::super::language_keyed::LanguageKeyed;
use super::super::runners;
use super::super::targets::resolve_target_operands;
use super::{PlannedSelectors, planned_current, planned_from_selector_plan};

pub(super) fn plan_explicit_target_selectors(
    repo_root: &std::path::Path,
    targets: &[String],
    ignore: &[String],
    extras: LanguageKeyed<&[String]>,
    lang_filter: Option<Language>,
    include_prior_failures: bool,
) -> Result<PlannedSelectors, String> {
    let query = resolve_target_operands(repo_root, targets, lang_filter, ignore, extras.python)
        .map_err(|e| format!("error: kiss test: {e}"))?;
    let mut source_paths = Vec::new();
    let mut changed_lines: BTreeMap<PathBuf, BTreeSet<u32>> = BTreeMap::new();
    for language in crate::test_runner::lang_registry::languages() {
        let files = query.source_files(language);
        rules_for(language).validate_explicit_targets(repo_root, &files, query.direct(language))?;
        source_paths.extend(files);
        for (path, lines) in query.lines(language) {
            changed_lines
                .entry(path.clone())
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    source_paths.sort();
    source_paths.dedup();
    let direct = LanguageKeyed::from_fn(|language| {
        query.direct(language).iter().cloned().collect::<Vec<_>>()
    });
    if source_paths.is_empty() {
        return Ok(planned_current(
            repo_root,
            ignore,
            direct,
            LanguageKeyed::default(),
            None,
        ));
    }
    let input = runners::CombinedSelectorInput {
        repo_root,
        source_paths: &source_paths,
        test_paths: &[],
        changed_lines: &changed_lines,
        test_args: extras,
        lang_filter,
        ignore,
        extra_direct: direct.as_slices(),
        include_prior_failures,
    };
    let selector_plan = runners::combined_selectors_with_direct(input)?;
    Ok(planned_from_selector_plan(
        repo_root.to_path_buf(),
        selector_plan,
        ignore.to_vec(),
    ))
}
