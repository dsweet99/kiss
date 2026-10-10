#![cfg_attr(not(test), allow(dead_code))]
pub(crate) use super::kernel_hooks::KernelHooks;
use super::runtime::EnsureRequest;

pub(crate) struct AllModePlan {
    pub planned: Vec<String>,
    pub population_required: bool,
}

pub(crate) trait KernelRules: KernelHooks {
    fn identity_stage(&self) -> &'static str;

    fn list_workspace_selectors(
        &self,
        repo_root: &std::path::Path,
        ignore: &[String],
        extras: &[String],
    ) -> Result<Vec<String>, String>;

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
