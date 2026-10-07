pub(crate) use super::kernel_hooks::KernelHooks;
use super::runtime::EnsureRequest;
use super::timing::session_timing_context_digest;
use super::witness::ExecutionWitness;
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) struct AllModePlan {
    pub planned: Vec<String>,
    pub population_required: bool,
}

pub(crate) trait KernelRules: KernelHooks {
    fn identity_stage(&self) -> &'static str;

    fn runner_identity_part(&self, repo_root: &std::path::Path) -> Option<serde_json::Value>;

    fn generation_ids(&self, repo_root: &std::path::Path) -> super::GenerationIds;

    fn stored_witness(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
    ) -> Option<ExecutionWitness>;

    fn stored_witness_matches_extras(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
    ) -> bool {
        let _ = repo_root;
        extras.is_empty()
    }

    fn list_workspace_selectors(
        &self,
        repo_root: &std::path::Path,
        ignore: &[String],
        extras: &[String],
    ) -> Result<Vec<String>, String>;

    fn live_misses(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        _identity: &str,
        witness: Option<&ExecutionWitness>,
    ) -> Vec<String> {
        if request.force {
            return planned.to_vec();
        }
        super::records::record_misses(planned, witness)
    }

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

    fn cancel_active_work(&self) {}

    fn all_mode_plan(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
        selectors: Vec<String>,
        gate: &kiss::GateConfig,
    ) -> AllModePlan;

    fn all_mode_skips_index_rebuild(&self) -> bool {
        false
    }

    fn reclaim_unreferenced(&self, repo_root: &std::path::Path) {
        let _ = repo_root;
    }

    fn accepted_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> Result<SelectorExecutionSummary, String> {
        Ok(self.cached_witness_summary(request, planned, witness))
    }

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
