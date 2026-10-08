use std::collections::BTreeMap;
use std::path::PathBuf;

use kiss::GateConfig;
use kiss::Language;
use kiss::test_records::TestRecord;

use super::witness::AcceptMode;
#[cfg(test)]
use super::witness::ExecutionWitness;
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};
use crate::test_runner::test_selection::SupportedLanguage;

#[derive(Clone, Debug)]
pub(crate) struct EnsureRequest {
    pub(crate) repo_root: PathBuf,
    pub(crate) mode: AcceptMode,
    pub(crate) lang_filter: Option<Language>,
    #[allow(dead_code)]
    pub(crate) ignore: Vec<String>,
    pub(crate) force: bool,
    pub(crate) force_selectors: Vec<String>,
    pub(crate) jobs: usize,
    pub(crate) gate: GateConfig,
    pub(crate) extras: LanguageKeyed<Vec<String>>,
    pub(crate) planned: LanguageKeyed<Vec<String>>,
}

impl EnsureRequest {
    pub(crate) fn planned_for(&self, language: Language) -> &[String] {
        self.planned.planned_for(language)
    }

    pub(crate) fn requires(&self, language: Language) -> bool {
        match self.lang_filter {
            Some(filter) if filter != language => return false,
            _ => {}
        }
        if !self.planned_for(language).is_empty() {
            return true;
        }
        matches!(self.mode, AcceptMode::All)
            && (self.lang_filter.is_none() || self.lang_filter == Some(language))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct OutcomeBatch {
    pub(crate) summary: SelectorExecutionSummary,
    pub(crate) selectors: Vec<String>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct LanguageEnsureResult {
    pub(crate) summary: SelectorExecutionSummary,
    pub(crate) published: bool,
    pub(crate) generation_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EnsureRuntimeResult {
    pub(crate) by_language: LanguageKeyed<Option<LanguageEnsureResult>>,
    pub(crate) exit_code: i32,
}

impl EnsureRuntimeResult {
    pub(crate) fn get(&self, language: kiss::Language) -> Option<&LanguageEnsureResult> {
        self.by_language.get(language).as_ref()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Listing {
    pub(crate) ids: Vec<String>,
    pub(crate) identity: String,
    pub(crate) record_identity: String,
}

pub(crate) trait LanguageRuntime: SupportedLanguage {
    fn list(&self, request: &EnsureRequest) -> Result<Listing, String>;

    fn deps(&self, request: &EnsureRequest, row: &TestRecord) -> Option<BTreeMap<String, String>>;

    fn run(
        &self,
        request: &EnsureRequest,
        ids: &[String],
        on_result: &mut dyn FnMut(SelectorExecutionRecord),
    ) -> Result<OutcomeBatch, String>;

    #[cfg(test)]
    fn seeded_rows(&self, _request: &EnsureRequest) -> Option<ExecutionWitness> {
        None
    }
}
