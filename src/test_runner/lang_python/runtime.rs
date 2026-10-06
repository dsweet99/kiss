use std::path::Path;

use kiss::Language;

use crate::test_runner::lang_iface::{
    EnsureRequest, ExecutionWitness, LanguageRuntime, Listing, OutcomeBatch,
};
use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};

pub(crate) struct PythonRuntime;

pub(crate) struct PythonKernelRules;

impl crate::test_runner::lang_iface::KernelRules for PythonKernelRules {
    fn identity_stage(&self) -> &'static str {
        "python_source_fingerprint"
    }

    fn all_mode_plan(
        &self,
        repo_root: &Path,
        extras: &[String],
        selectors: Vec<String>,
        _gate: &kiss::GateConfig,
    ) -> crate::test_runner::lang_iface::AllModePlan {
        let (planned, population_required) =
            super::all_mode_plan::python_all_plan(repo_root, extras, selectors);
        crate::test_runner::lang_iface::AllModePlan {
            planned,
            population_required,
        }
    }

    fn all_mode_skips_index_rebuild(&self) -> bool {
        true
    }

    fn live_misses(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        identity: &str,
        _witness: Option<&ExecutionWitness>,
    ) -> Vec<String> {
        crate::test_runner::lang_iface::miss_selectors_for_repair(
            request.mode,
            planned,
            identity,
            None,
            request.force,
        )
    }

    fn stored_coverage(&self, repo_root: &Path) -> crate::test_runner::lang_iface::StoredCoverage {
        super::stored::stored_coverage(repo_root)
    }

    fn runner_identity_part(&self, _repo_root: &Path) -> Option<serde_json::Value> {
        None
    }

    fn generation_ids(&self, repo_root: &Path) -> crate::test_runner::lang_iface::GenerationIds {
        super::stored::generation_ids(repo_root)
    }

    fn stored_witness(&self, repo_root: &Path, extras: &[String]) -> Option<ExecutionWitness> {
        if !kiss::rslip::python_records_dir(repo_root).is_dir() {
            return None;
        }
        super::stored::stored_witness(repo_root, extras)
    }

    fn stored_witness_matches_extras(&self, _repo_root: &Path, _extras: &[String]) -> bool {
        true
    }

    fn historical_covering_selectors(
        &self,
        repo_root: &Path,
        keys: &[String],
        abs: &[std::path::PathBuf],
    ) -> std::collections::BTreeSet<String> {
        super::stored::historical_covering_selectors(repo_root, keys, abs)
    }

    fn indexes_path(&self, repo_root: &Path, keys: &[String]) -> bool {
        super::stored::indexes_path(repo_root, keys)
    }

    fn is_test_source(&self, path: &Path) -> bool {
        kiss::is_python_test_module_path(path)
    }

    fn list_workspace_selectors(
        &self,
        repo_root: &Path,
        ignore: &[String],
        extras: &[String],
    ) -> Result<Vec<String>, String> {
        use crate::test_runner::workspace_selector_cache as selector_cache;
        if let Some(cached) =
            selector_cache::load_cached_python_workspace_selectors(repo_root, ignore, extras)
        {
            return Ok(cached);
        }
        let out = crate::test_runner::runners::enumerate_workspace_python_selectors(
            repo_root, ignore, extras,
        );
        if let Ok(ids) = out.as_ref() {
            selector_cache::store_python_workspace_selectors(repo_root, ignore, ids, extras);
            crate::test_runner::target_request::add_index();
        }
        out
    }

    fn cancel_active_work(&self) {
        kiss::rpytest_runner::cancel_active_forkservers();
    }
}

impl LanguageRuntime for PythonRuntime {
    fn list(&self, request: &EnsureRequest) -> Result<Listing, String> {
        let record_identity =
            super::stored::record_identity(&request.repo_root, &request.extras.python)
                .ok_or("error: kiss: python runner identity unavailable")?;
        Ok(Listing {
            ids: request.planned.python.clone(),
            identity: format!("py:{record_identity}"),
            record_identity,
        })
    }

    fn deps(
        &self,
        request: &EnsureRequest,
        row: &kiss::test_records::TestRecord,
    ) -> Option<std::collections::BTreeMap<String, String>> {
        super::stored::current_deps(&request.repo_root, row)
    }

    fn run(
        &self,
        request: &EnsureRequest,
        ids: &[String],
        on_result: &mut dyn FnMut(SelectorExecutionRecord),
    ) -> Result<OutcomeBatch, String> {
        run_python_selectors(request, ids, on_result)
    }
}

pub(super) fn run_python_selectors(
    request: &EnsureRequest,
    miss_set: &[String],
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<OutcomeBatch, String> {
    if miss_set.is_empty() {
        return Ok(OutcomeBatch::default());
    }
    super::rslip::run_rslip_selectors_streaming(
        super::rslip::RslipSelectorsArgs {
            repo_root: &request.repo_root,
            selectors: miss_set,
            extra: &request.extras.python,
            force_rerun: request.force,
            force_rerun_selectors: &request.force_selectors,
            jobs: request.jobs,
            gate: &request.gate,
        },
        on_result,
    )?;
    Ok(OutcomeBatch {
        summary: SelectorExecutionSummary::default(),
        selectors: miss_set.to_vec(),
    })
}

impl crate::test_runner::coverage_decision::SupportedLanguage for PythonRuntime {
    fn language(&self) -> Language {
        Language::Python
    }
}
