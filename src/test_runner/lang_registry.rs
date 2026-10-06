use kiss::Language;

use crate::test_runner::lang_iface::KernelRules;

pub(crate) fn rules_for(language: Language) -> &'static dyn KernelRules {
    match language {
        Language::Python => &crate::test_runner::lang_python::PythonKernelRules,
        Language::Rust => &crate::test_runner::lang_rust::RustKernelRules,
    }
}

pub(crate) fn execution_module(
    language: Language,
    planned: &crate::test_runner::PlannedSelectors,
    options: &crate::test_runner::SelectorRunOptions<'_>,
) -> Box<dyn crate::test_runner::coverage_decision::LanguageTestModule> {
    match language {
        Language::Python => Box::new(
            crate::test_runner::lang_python::backer::PythonModule::for_execution_with_args(
                &planned.repo_root,
                &planned.ignore,
                options.extras.python,
            ),
        ),
        Language::Rust => Box::new(
            crate::test_runner::lang_rust::backer::RustModule::for_execution(
                &planned.repo_root,
                &planned.ignore,
            ),
        ),
    }
}

pub(crate) struct PlannerBackerInput<'a> {
    pub(crate) repo_root: &'a std::path::Path,
    pub(crate) source_paths: &'a [std::path::PathBuf],
    pub(crate) changed_lines:
        &'a std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<u32>>,
    pub(crate) test_args: &'a [String],
    pub(crate) ignore: &'a [String],
    pub(crate) changed_tests: &'a [crate::test_runner::coverage_decision::TestSelector],
    pub(crate) prior_failures: &'a [crate::test_runner::coverage_decision::TestSelector],
}

pub(crate) fn planner_backer(
    language: Language,
    input: PlannerBackerInput<'_>,
) -> Box<dyn crate::test_runner::coverage_decision::LanguagePlanner> {
    match language {
        Language::Python => crate::test_runner::lang_python::backer::python_population_backer(
            input.repo_root,
            input.source_paths,
            input.changed_lines,
            input.test_args,
            input.ignore,
            input.changed_tests,
            input.prior_failures,
        ),
        Language::Rust => crate::test_runner::lang_rust::backer::rust_backer(
            crate::test_runner::lang_rust::backer::RustBackerInput {
                repo_root: input.repo_root,
                rust_source_paths: input.source_paths,
                ignore: input.ignore,
                changed_tests: input.changed_tests,
                prior_failures: input.prior_failures,
            },
        ),
    }
}

pub(crate) fn cached_workspace_selectors(
    repo_root: &std::path::Path,
    language: Language,
    ignore: &[String],
    test_args: &[String],
) -> Result<Vec<String>, String> {
    use crate::test_runner::workspace_selector_cache as cache;
    let cached = match language {
        Language::Python => {
            cache::load_cached_python_workspace_selectors(repo_root, ignore, test_args)
                .map(|selectors| selectors.into_iter().collect())
        }
        Language::Rust => cache::load_cached_rust_workspace_selectors(repo_root, ignore)
            .map(|selectors| selectors.into_iter().collect()),
    };
    match cached {
        Some(selectors) => Ok(selectors),
        None => enumerate_workspace_selectors(repo_root, language, ignore, test_args),
    }
}

pub(crate) fn enumerate_workspace_selectors(
    repo_root: &std::path::Path,
    language: Language,
    ignore: &[String],
    test_args: &[String],
) -> Result<Vec<String>, String> {
    match language {
        Language::Python => crate::test_runner::runners::enumerate_workspace_python_selectors(
            repo_root, ignore, test_args,
        ),
        Language::Rust => {
            crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
        }
    }
}

pub(crate) fn languages() -> [Language; 2] {
    Language::ALL
}

pub(crate) fn all_rules() -> impl Iterator<Item = &'static dyn KernelRules> {
    languages().into_iter().map(rules_for)
}

pub(crate) fn language_for_extension(ext: &str) -> Option<Language> {
    Language::from_extension(ext)
}

pub(crate) fn language_for_label(label: &str) -> Option<Language> {
    Language::from_label(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_for_maps_each_language_to_its_identity_stage() {
        assert_eq!(
            rules_for(Language::Python).identity_stage(),
            "python_source_fingerprint"
        );
        assert_eq!(rules_for(Language::Rust).identity_stage(), "rust_identity");
        assert!(
            rules_for(Language::Python)
                .validate_extra_args(&["--bogus".into()])
                .is_ok()
        );
    }
}
