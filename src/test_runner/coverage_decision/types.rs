use std::cmp::Ordering;

use kiss::Language;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TestSelector {
    pub(crate) language: Language,
    pub(crate) id: String,
}

impl TestSelector {
    pub(crate) fn new(language: Language, id: impl Into<String>) -> Self {
        Self {
            language,
            id: id.into(),
        }
    }
}

impl PartialOrd for TestSelector {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TestSelector {
    fn cmp(&self, other: &Self) -> Ordering {
        language_sort_key(self.language)
            .cmp(&language_sort_key(other.language))
            .then_with(|| self.id.cmp(&other.id))
    }
}

pub(crate) fn language_sort_key(language: Language) -> u8 {
    match language {
        Language::Python => 0,
        Language::Rust => 1,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedSource {
    pub(crate) language: Language,
    pub(crate) path: String,
}

impl ChangedSource {
    pub(crate) fn new(language: Language, path: impl Into<String>) -> Self {
        Self {
            language,
            path: path.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedTestSelector {
    pub(crate) selector: TestSelector,
}

impl ChangedTestSelector {
    pub(crate) fn new(selector: TestSelector) -> Self {
        Self { selector }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedDiff {
    pub(crate) sources: Vec<ChangedSource>,
}

impl ChangedDiff {
    pub(crate) fn new(sources: Vec<ChangedSource>) -> Self {
        Self { sources }
    }

    pub(crate) fn sources_for_language(&self, language: Language) -> Vec<ChangedSource> {
        self.sources
            .iter()
            .filter(|source| source.language == language)
            .cloned()
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoverageFreshness {
    Fresh,
    Stale,
    Unknown,
}

impl CoverageFreshness {
    pub(crate) fn requires_population(self) -> bool {
        matches!(self, Self::Stale | Self::Unknown)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectionDecision {
    pub(crate) selectors: Vec<TestSelector>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PopulationPlan {
    pub(crate) selectors: Vec<TestSelector>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoverageDecisionPlan {
    pub(crate) selected: Vec<TestSelector>,
    pub(crate) population: Vec<TestSelector>,
    pub(crate) population_languages: Vec<Language>,
}
