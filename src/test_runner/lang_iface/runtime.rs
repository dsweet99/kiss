//! `LanguageRuntime` and ensure request/result types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiss::Language;

use super::witness::{AcceptMode, ExecutionWitness, WitnessStatus};
use crate::test_runner::runners::SelectorExecutionSummary;

#[derive(Clone, Debug)]
pub(crate) struct EnsureRequest {
    pub(crate) repo_root: PathBuf,
    pub(crate) mode: AcceptMode,
    pub(crate) lang_filter: Option<Language>,
    #[allow(dead_code)] // reserved for future ignore-aware discovery in LanguageRuntime
    pub(crate) ignore: Vec<String>,
    pub(crate) force: bool,
    pub(crate) jobs: usize,
    pub(crate) python_extra: Vec<String>,
    pub(crate) rust_extra: Vec<String>,
    pub(crate) planned_python: Vec<String>,
    pub(crate) planned_rust: Vec<String>,
}

impl EnsureRequest {
    pub(crate) fn planned_for(&self, language: Language) -> &[String] {
        match language {
            Language::Python => &self.planned_python,
            Language::Rust => &self.planned_rust,
        }
    }

    /// Whether this request includes `language`.
    ///
    /// Empty planned All-mode still requires the module so the kernel can publish
    /// an empty Full witness (seeded cov fixtures / empty repos).
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
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<u64>,
    pub(crate) covered_lines: BTreeMap<String, Vec<u32>>,
    /// Full planned universe for Full publication; None means delta/repair only.
    pub(crate) publication_universe: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub(crate) struct PublishBatch {
    pub(crate) selectors: Vec<String>,
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<u64>,
    pub(crate) covered_lines: BTreeMap<String, Vec<u32>>,
    pub(crate) publication_universe: Option<Vec<String>>,
    #[allow(dead_code)]
    pub(crate) summary: SelectorExecutionSummary,
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
    pub(crate) python: Option<LanguageEnsureResult>,
    pub(crate) rust: Option<LanguageEnsureResult>,
    pub(crate) exit_code: i32,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // LanguageRuntime contract surface
pub(crate) struct CoverageSnapshot {
    pub(crate) covered_lines: BTreeMap<String, Vec<u32>>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // LanguageRuntime contract surface
pub(crate) struct StatusTimingSnapshot {
    pub(crate) selectors: Vec<String>,
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<u64>,
}

/// Per-language cache/run/publish policy for the shared ensure kernel.
#[allow(dead_code)] // contract surface for kernel + future reporters
pub(crate) trait LanguageRuntime {
    fn language(&self) -> Language;

    fn discover_universe(&self, request: &EnsureRequest) -> Result<Vec<String>, String> {
        Ok(request.planned_for(self.language()).to_vec())
    }

    fn current_identity(&self, request: &EnsureRequest) -> Result<String, String>;

    fn load_full_witness(&self, repo_root: &Path) -> Result<ExecutionWitness, String>;

    fn run_selectors(
        &self,
        request: &EnsureRequest,
        miss_set: &[String],
    ) -> Result<OutcomeBatch, String>;

    fn publish_outcomes(
        &self,
        request: &EnsureRequest,
        batch: &PublishBatch,
    ) -> Result<(), String>;

    fn coverage_snapshot(&self, repo_root: &Path) -> Result<CoverageSnapshot, String> {
        let witness = self.load_full_witness(repo_root)?;
        Ok(CoverageSnapshot {
            covered_lines: witness.covered_lines,
        })
    }

    fn status_timing_snapshot(&self, repo_root: &Path) -> Result<StatusTimingSnapshot, String> {
        let witness = self.load_full_witness(repo_root)?;
        Ok(StatusTimingSnapshot {
            selectors: witness.selectors,
            statuses: witness.statuses,
            durations_ns: witness.durations_ns,
        })
    }

    fn is_indexable_source(&self, path: &Path, repo_root: &Path) -> bool;

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        extra: &[String],
        jobs: usize,
    ) -> Result<Vec<String>, String>;

    fn accepted_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary;
}
