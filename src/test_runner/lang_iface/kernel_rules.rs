use super::runtime::EnsureRequest;
use super::timing::session_timing_context_digest;
use super::witness::{ExecutionWitness, miss_selectors_for_repair};
use super::witness_summary::summary_from_witness_statuses;
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) struct AllModePlan {
    pub planned: Vec<String>,
    pub population_required: bool,
}

/// Per-language policy that shared planning, pipeline, and ensure code consults
/// instead of branching on `Language`.
pub(crate) trait KernelRules: Sync {
    fn identity_stage(&self) -> &'static str;

    /// This language's part of the runner identity; `None` while no runner is recorded.
    fn runner_identity_part(&self, repo_root: &std::path::Path) -> Option<serde_json::Value>;

    fn generation_ids(&self, repo_root: &std::path::Path) -> super::GenerationIds;

    /// The stored witness for a run with runner arguments `extras`.
    fn stored_witness(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
    ) -> Option<ExecutionWitness>;

    /// Whether the rows of [`Self::stored_witness`] can stand for a run with runner
    /// arguments `extras`.
    fn stored_witness_matches_extras(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
    ) -> bool {
        let _ = repo_root;
        extras.is_empty()
    }

    /// Display labels for report selectors whose ids are not human-readable.
    fn report_labels(
        &self,
        repo_root: &std::path::Path,
        selectors: &[String],
    ) -> std::collections::BTreeMap<String, String> {
        let _ = (repo_root, selectors);
        std::collections::BTreeMap::new()
    }

    /// Whether `path` is test code rather than production source.
    fn is_test_source(&self, path: &std::path::Path) -> bool {
        let _ = path;
        false
    }

    /// Every test id in the workspace, from the selector cache when it is current.
    fn list_workspace_selectors(
        &self,
        repo_root: &std::path::Path,
        ignore: &[String],
        extras: &[String],
    ) -> Result<Vec<String>, String>;

    /// Prefix for the kernel's per-step stage timings; `None` emits none.
    fn stage_prefix(&self) -> Option<&'static str> {
        None
    }

    fn live_misses(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        identity: &str,
        witness: Option<&ExecutionWitness>,
    ) -> Vec<String> {
        miss_selectors_for_repair(request.mode, planned, identity, witness, request.force)
    }

    fn time_gate_selector_error_is_fatal(&self) -> bool {
        true
    }

    /// Whether every stored witness selector that did not miss counts as cached.
    fn recap_stored_selectors(&self) -> bool {
        false
    }

    fn current_timing_digest(&self, request: &EnsureRequest) -> String {
        let _ = request;
        session_timing_context_digest(0)
    }

    fn stored_timing_digest(&self, request: &EnsureRequest) -> String {
        let _ = request;
        session_timing_context_digest(0)
    }

    /// Stops this language's in-flight test processes after a peer language failed.
    fn cancel_active_work(&self) {}

    fn validate_extra_args(&self, extras: &[String]) -> Result<(), String> {
        let _ = extras;
        Ok(())
    }

    /// Plan for running every discovered test of this language.
    fn all_mode_plan(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
        selectors: Vec<String>,
        gate: &kiss::GateConfig,
    ) -> AllModePlan;

    /// Rejects explicit targets this language cannot cover; `files` are absolute paths.
    fn validate_explicit_targets(
        &self,
        repo_root: &std::path::Path,
        files: &[std::path::PathBuf],
        direct: &std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        let _ = (repo_root, files, direct);
        Ok(())
    }

    /// Whether these extra arguments rule out the automatic cold population.
    fn extras_block_cold_population(&self, extras: &[String]) -> bool {
        let _ = extras;
        false
    }

    /// Whether an all-mode plan skips the index rebuild after a selective run.
    fn all_mode_skips_index_rebuild(&self) -> bool {
        false
    }

    /// Summary of planned selectors answered entirely from the stored witness.
    fn cached_witness_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        let _ = request;
        summary_from_witness_statuses(planned, witness, |selector| selector.to_string(), false)
    }

    /// Removes stored state that nothing references, before a run.
    fn reclaim_unreferenced(&self, repo_root: &std::path::Path) {
        let _ = repo_root;
    }

    /// Summary when every planned selector is accepted without running.
    fn accepted_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> Result<SelectorExecutionSummary, String> {
        Ok(self.cached_witness_summary(request, planned, witness))
    }

    /// Selector names the time gate matches against `max_unit_test_seconds` keys.
    fn selectors_for_time_gate(
        &self,
        request: &EnsureRequest,
        selectors: &[String],
    ) -> Result<Vec<String>, String> {
        let _ = request;
        Ok(selectors.to_vec())
    }

    fn bind_subprocess_observer(&self, request: &EnsureRequest) {
        let _ = request;
        kiss::subprocess_observer::reset_subprocess_observer();
    }
}

pub(crate) fn emit_kernel_stage(rules: &dyn KernelRules, name: &str, started: std::time::Instant) {
    if let Some(prefix) = rules.stage_prefix() {
        crate::test_runner::emit_stage_time(&format!("{prefix}_{name}"), started.elapsed());
    }
}
