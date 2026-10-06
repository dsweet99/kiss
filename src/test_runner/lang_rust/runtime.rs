use kiss::Language;

use crate::test_runner::lang_iface::{
    EnsureRequest, ExecutionWitness, LanguageRuntime, Listing, OutcomeBatch,
};
use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};
use crate::test_runner::selector_ids::{
    qualified_rust_report_ids, report_string_for_logical_string,
};

use super::records_witness::{known_selectors, rust_witness_identity};

#[derive(Default)]
pub(crate) struct RustRuntime {
    known_selectors: std::cell::OnceCell<Option<std::collections::BTreeSet<String>>>,
    current_deps: std::cell::OnceCell<Option<super::nextest::CurrentDeps>>,
}

pub(super) fn rust_summary_from_witness_statuses(
    request: &EnsureRequest,
    planned: &[String],
    witness: &ExecutionWitness,
) -> SelectorExecutionSummary {
    let report_ids = if kiss::time_gate_uses_path_prefixes(&request.gate.max_unit_test_seconds) {
        crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
            &request.repo_root,
            &[],
        )
        .unwrap_or_default()
    } else {
        qualified_rust_report_ids(&request.repo_root, planned)
    };
    crate::test_runner::lang_iface::summary_from_witness_statuses(
        planned,
        witness,
        |selector| report_string_for_logical_string(&report_ids, selector),
        false,
    )
}

impl LanguageRuntime for RustRuntime {
    fn list(&self, request: &EnsureRequest) -> Result<Listing, String> {
        let record_identity =
            super::nextest::record_identity(&request.repo_root, &request.extras.rust)?;
        Ok(Listing {
            ids: request.planned.rust.clone(),
            identity: rust_witness_identity(&record_identity),
            record_identity,
        })
    }

    fn deps(
        &self,
        request: &EnsureRequest,
        row: &kiss::test_records::TestRecord,
    ) -> Option<std::collections::BTreeMap<String, String>> {
        let known = self
            .known_selectors
            .get_or_init(|| known_selectors(&request.repo_root));
        if known
            .as_ref()
            .is_some_and(|known| !known.contains(&row.test_id))
        {
            return None;
        }
        let deps = self.current_deps.get_or_init(|| {
            super::nextest::CurrentDeps::new(&request.repo_root, &request.gate).ok()
        });
        Some(deps.as_ref()?.of(row))
    }

    #[cfg(test)]
    fn seeded_rows(&self, request: &EnsureRequest) -> Option<ExecutionWitness> {
        super::test_records::load(&request.repo_root)
    }

    fn run(
        &self,
        request: &EnsureRequest,
        ids: &[String],
        on_result: &mut dyn FnMut(SelectorExecutionRecord),
    ) -> Result<OutcomeBatch, String> {
        run_rust_selectors(request, ids, on_result)
    }
}

pub(super) fn run_rust_selectors(
    request: &EnsureRequest,
    ids: &[String],
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<OutcomeBatch, String> {
    let summary = super::nextest::run_nextest_selectors(
        &super::nextest::RunRequest {
            repo_root: &request.repo_root,
            selectors: ids,
            extras: &request.extras.rust,
            jobs: request.jobs,
            gate: &request.gate,
        },
        on_result,
    )?;
    Ok(OutcomeBatch {
        summary,
        selectors: ids.to_vec(),
    })
}

impl crate::test_runner::coverage_decision::SupportedLanguage for RustRuntime {
    fn language(&self) -> Language {
        Language::Rust
    }
}
