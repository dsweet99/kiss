use crate::test_runner::lang_iface::{EnsureRequest, KernelHooks, KernelRules};

pub(crate) struct RustKernelRules;

impl KernelHooks for RustKernelRules {
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
}

impl KernelRules for RustKernelRules {
    fn identity_stage(&self) -> &'static str {
        "rust_identity"
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
