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
        self.language
            .cmp(&other.language)
            .then_with(|| self.id.cmp(&other.id))
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
pub(crate) struct ChangedDiff {
    pub(crate) sources: Vec<ChangedSource>,
}

impl ChangedDiff {
    pub(crate) fn new(sources: Vec<ChangedSource>) -> Self {
        Self { sources }
    }

    #[cfg(test)]
    pub(crate) fn sources_for_language(&self, language: Language) -> Vec<ChangedSource> {
        self.sources
            .iter()
            .filter(|source| source.language == language)
            .cloned()
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SelectionBasis {
    #[default]
    Current,
    Population,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectionPlan {
    pub(crate) selected: Vec<TestSelector>,
    pub(crate) population: Vec<TestSelector>,
    pub(crate) population_languages: Vec<Language>,
}
