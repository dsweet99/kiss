use crate::test_runner::lang_iface::{EnsureRequest, ExecutionWitness, KernelRules};
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) struct RustKernelRules;

impl KernelRules for RustKernelRules {
    fn identity_stage(&self) -> &'static str {
        "rust_identity"
    }

    fn stored_coverage(
        &self,
        repo_root: &std::path::Path,
    ) -> crate::test_runner::lang_iface::StoredCoverage {
        super::stored::stored_coverage(repo_root)
    }

    fn runner_identity_part(&self, repo_root: &std::path::Path) -> Option<serde_json::Value> {
        super::stored::runner_identity_part(repo_root)
    }

    fn generation_ids(
        &self,
        repo_root: &std::path::Path,
    ) -> crate::test_runner::lang_iface::GenerationIds {
        super::stored::generation_ids(repo_root)
    }

    fn stored_witness(
        &self,
        repo_root: &std::path::Path,
        _extras: &[String],
    ) -> Option<ExecutionWitness> {
        super::stored::stored_witness(repo_root)
    }

    fn historical_covering_selectors(
        &self,
        repo_root: &std::path::Path,
        keys: &[String],
        abs: &[std::path::PathBuf],
    ) -> std::collections::BTreeSet<String> {
        super::stored::historical_covering_selectors(repo_root, keys, abs)
    }

    fn indexes_path(&self, repo_root: &std::path::Path, keys: &[String]) -> bool {
        super::stored::indexes_path(repo_root, keys)
    }

    fn report_labels(
        &self,
        repo_root: &std::path::Path,
        selectors: &[String],
    ) -> std::collections::BTreeMap<String, String> {
        crate::test_runner::selector_ids::qualified_rust_report_ids(repo_root, selectors)
    }

    fn list_workspace_selectors(
        &self,
        repo_root: &std::path::Path,
        ignore: &[String],
        _extras: &[String],
    ) -> Result<Vec<String>, String> {
        use crate::test_runner::workspace_selector_cache as selector_cache;
        if let Some(cached) =
            selector_cache::load_cached_rust_workspace_selectors(repo_root, ignore)
        {
            return Ok(cached);
        }
        let out =
            crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore);
        if let Ok(ids) = out.as_ref() {
            selector_cache::store_rust_workspace_selectors(repo_root, ignore, ids);
            crate::test_runner::target_request::add_index();
        }
        out
    }

    fn all_mode_plan(
        &self,
        repo_root: &std::path::Path,
        _extras: &[String],
        selectors: Vec<String>,
        gate: &kiss::GateConfig,
    ) -> crate::test_runner::lang_iface::AllModePlan {
        let plan = super::all_mode_plan::rust_plan_selectors(repo_root, selectors, gate);
        crate::test_runner::lang_iface::AllModePlan {
            planned: plan.planned,
            population_required: plan.population_required,
        }
    }

    fn validate_explicit_targets(
        &self,
        repo_root: &std::path::Path,
        files: &[std::path::PathBuf],
        direct: &std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        super::workspace::reject_non_member_rust_targets(repo_root, files, direct)
    }

    fn extras_block_cold_population(&self, extras: &[String]) -> bool {
        !extras.is_empty()
    }

    fn stage_prefix(&self) -> Option<&'static str> {
        Some("rust")
    }

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
        super::records_witness::record_misses(planned, witness)
    }

    fn time_gate_selector_error_is_fatal(&self) -> bool {
        false
    }

    fn recap_stored_selectors(&self) -> bool {
        true
    }

    fn cancel_active_work(&self) {
        kiss::rust_llvm_cov_runner::cancel_active_batch_scope();
    }

    fn begin_covering(
        &self,
        repo_root: &std::path::Path,
        extras: &[String],
        jobs: usize,
        dry_run: bool,
    ) -> Option<Box<dyn std::any::Any>> {
        let _ = (repo_root, extras, jobs, dry_run);
        kiss::rust_llvm_cov_runner::begin_identity_memo();
        None
    }

    fn validate_extra_args(&self, extras: &[String]) -> Result<(), String> {
        crate::test_runner::rust_llvm_cov::validate_rust_extra_args(extras)
    }

    fn cached_witness_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        let planned = super::witness_identity::rust_witness_overlap(planned, witness);
        super::runtime::rust_summary_from_witness_statuses(request, &planned, witness)
    }

    fn accepted_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> Result<SelectorExecutionSummary, String> {
        super::runtime::rust_accepted_summary(request, planned, witness)
    }

    fn selectors_for_time_gate(
        &self,
        request: &EnsureRequest,
        selectors: &[String],
    ) -> Result<Vec<String>, String> {
        if !kiss::time_gate_uses_path_prefixes(&request.gate.max_unit_test_seconds) {
            return Ok(selectors.to_vec());
        }
        let report_ids =
            crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
                &request.repo_root,
                &[],
            )?;
        for selector in selectors {
            crate::test_runner::runners::require_kiss_test_report_id(&report_ids, selector)?;
        }
        Ok(
            crate::test_runner::selector_ids::report_strings_for_logical_strings(
                &report_ids,
                selectors,
            ),
        )
    }
}
