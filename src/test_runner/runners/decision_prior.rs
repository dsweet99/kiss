use std::path::Path;

use crate::test_runner::coverage_decision::TestSelector;

pub(crate) fn prior_failures_for_language(
    repo_root: &Path,
    language: kiss::Language,
) -> Vec<TestSelector> {
    kiss::test_records::nonpassed_test_ids(&kiss::test_records::records_dir(
        repo_root,
        language.label(),
    ))
    .into_iter()
    .map(|id| TestSelector::new(language, id))
    .collect()
}
