//! `LanguageRuntime` and ensure request/result types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiss::Language;
use kiss::GateConfig;

use super::witness::{AcceptMode, ExecutionWitness, WitnessStatus, summary_from_witness_statuses};
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::runners::SelectorExecutionSummary;

#[derive(Clone, Debug)]
pub(crate) struct EnsureRequest {
    pub(crate) repo_root: PathBuf,
    pub(crate) mode: AcceptMode,
    pub(crate) lang_filter: Option<Language>,
    #[allow(dead_code)] // reserved for future ignore-aware discovery in LanguageRuntime
    pub(crate) ignore: Vec<String>,
    pub(crate) force: bool,
    /// Selectors that must re-run even if the witness still says Passed (prior failures).
    pub(crate) force_selectors: Vec<String>,
    pub(crate) jobs: usize,
    /// Session gate for this ensure (loaded once by CLI / caller; do not reload).
    pub(crate) gate: GateConfig,
    /// Per-language CLI extras (pytest plugins / cargo test args) — not parallel product fields.
    pub(crate) extras: LanguageKeyed<Vec<String>>,
    /// Planned selectors keyed by language.
    pub(crate) planned: LanguageKeyed<Vec<String>>,
}

impl EnsureRequest {
    pub(crate) fn planned_for(&self, language: Language) -> &[String] {
        self.planned.planned_for(language)
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
    pub(crate) durations_ns: Vec<Option<u64>>,
    pub(crate) covered_lines: BTreeMap<String, Vec<u32>>,
    /// Full planned universe for Full publication; None means delta/repair only.
    pub(crate) publication_universe: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub(crate) struct PublishBatch {
    pub(crate) selectors: Vec<String>,
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<Option<u64>>,
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
    /// Per-language ensure outcomes (not parallel Option product fields).
    pub(crate) by_language: LanguageKeyed<Option<LanguageEnsureResult>>,
    pub(crate) exit_code: i32,
}

impl EnsureRuntimeResult {
    pub(crate) fn python(&self) -> Option<&LanguageEnsureResult> {
        self.by_language.python.as_ref()
    }

    pub(crate) fn rust(&self) -> Option<&LanguageEnsureResult> {
        self.by_language.rust.as_ref()
    }
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
    pub(crate) durations_ns: Vec<Option<u64>>,
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

    /// Report planned selectors from a witness without assuming all Passed.
    fn cached_witness_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        let _ = request;
        summary_from_witness_statuses(planned, witness, |selector| selector.to_string(), false)
    }

    /// Selectors used when applying `max_unit_test_seconds` during accept.
    /// Rust witnesses store nextest logical ids; time-gate patterns expect PATH::symbol.
    fn selectors_for_time_gate(
        &self,
        _request: &EnsureRequest,
        selectors: &[String],
    ) -> Result<Vec<String>, String> {
        Ok(selectors.to_vec())
    }
}
