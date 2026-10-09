use crate::test_runner::lang_iface::{EnsureRequest, ExecutionWitness, KernelHooks, KernelRules};
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) struct RustKernelRules;

impl KernelHooks for RustKernelRules {
    fn report_labels(
        &self,
        repo_root: &std::path::Path,
        selectors: &[String],
    ) -> std::collections::BTreeMap<String, String> {
        crate::test_runner::selector_ids::qualified_rust_report_ids(repo_root, selectors)
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

    fn validate_extra_args(&self, extras: &[String]) -> Result<(), String> {
        super::nextest::validate_rust_extra_args(extras)
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
}

impl KernelRules for RustKernelRules {
    fn identity_stage(&self) -> &'static str {
        "rust_identity"
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
        extras: &[String],
    ) -> Option<ExecutionWitness> {
        super::stored::stored_witness(repo_root, extras)
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
        _gate: &kiss::GateConfig,
    ) -> crate::test_runner::lang_iface::AllModePlan {
        crate::test_runner::lang_iface::records::records_all_mode_plan(repo_root, "rust", selectors)
    }

    fn cancel_active_work(&self) {
        super::nextest::cancel_active_run();
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
