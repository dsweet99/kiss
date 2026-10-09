#![cfg_attr(not(test), allow(dead_code))]
use std::path::Path;

use kiss::Language;

use crate::test_runner::lang_iface::{
    EnsureRequest, ExecutionWitness, LanguageRuntime, Listing, OutcomeBatch,
};
use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};

#[derive(Default)]
pub(crate) struct PythonRuntime {
    current_deps: std::cell::OnceCell<Option<super::records::CurrentDeps>>,
}

pub(crate) struct PythonKernelRules;

impl crate::test_runner::lang_iface::KernelHooks for PythonKernelRules {
    fn is_test_source(&self, path: &Path) -> bool {
        kiss::is_python_test_module_path(path)
    }
}

impl crate::test_runner::lang_iface::KernelRules for PythonKernelRules {
    fn identity_stage(&self) -> &'static str {
        "python_source_fingerprint"
    }

    fn all_mode_plan(
        &self,
        repo_root: &Path,
        _extras: &[String],
        selectors: Vec<String>,
        _gate: &kiss::GateConfig,
    ) -> crate::test_runner::lang_iface::AllModePlan {
        crate::test_runner::lang_iface::records::records_all_mode_plan(
            repo_root, "python", selectors,
        )
    }

    fn runner_identity_part(&self, _repo_root: &Path) -> Option<serde_json::Value> {
        None
    }

    fn generation_ids(&self, repo_root: &Path) -> crate::test_runner::lang_iface::GenerationIds {
        super::stored::generation_ids(repo_root)
    }

    fn stored_witness(&self, repo_root: &Path, extras: &[String]) -> Option<ExecutionWitness> {
        if !kiss::test_records::records_dir(repo_root, "python").is_dir() {
            return None;
        }
        super::stored::stored_witness(repo_root, extras)
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
            super::records::record_identity(&request.repo_root, &request.extras.python)?;
        Ok(Listing {
            ids: request.planned.python.clone(),
            identity: super::stored::python_witness_identity(&record_identity),
            record_identity,
        })
    }

    fn deps(
        &self,
        request: &EnsureRequest,
        row: &kiss::test_records::TestRecord,
    ) -> Option<std::collections::BTreeMap<String, String>> {
        let deps = self.current_deps.get_or_init(|| {
            super::records::CurrentDeps::new(&request.repo_root, &request.gate).ok()
        });
        Some(deps.as_ref()?.of(row))
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
    super::run::run_pytest_selectors(
        &super::run::PytestSelectorsArgs {
            repo_root: &request.repo_root,
            selectors: miss_set,
            extra: &request.extras.python,
            force_rerun: request.force,
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

impl crate::test_runner::test_selection::SupportedLanguage for PythonRuntime {
    fn language(&self) -> Language {
        Language::Python
    }
}
