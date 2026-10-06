use kiss::Language;

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, Listing, OutcomeBatch,
};
use crate::test_runner::runners::{SelectorExecutionRecord, SelectorExecutionSummary};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, resolved_rust_batch_request_parts,
};
use crate::test_runner::selector_ids::{
    qualified_rust_report_ids, report_string_for_logical_string,
};

use super::records_witness::{known_selectors, rust_identity_digest_from_batch};
use super::witness_identity::rust_witness_overlap;

#[path = "population_repair.rs"]
mod population_repair;

#[derive(Default)]
pub(crate) struct RustRuntime {
    known_selectors: std::cell::OnceCell<Option<std::collections::BTreeSet<String>>>,
    current_deps: std::cell::RefCell<Option<kiss::rust_llvm_cov_runner::RustRecordDeps>>,
}

fn rust_population_publication_selectors(
    mode: AcceptMode,
    planned: &[String],
) -> Option<Vec<String>> {
    match mode {
        AcceptMode::All => Some(planned.to_vec()),
        AcceptMode::Subset => None,
    }
}

pub(super) fn rust_summary_from_witness_statuses(
    request: &EnsureRequest,
    planned: &[String],
    witness: &ExecutionWitness,
) -> SelectorExecutionSummary {
    if !kiss::time_gate_uses_path_prefixes(&request.gate.max_unit_test_seconds) {
        let report_ids = qualified_rust_report_ids(&request.repo_root, planned);
        return crate::test_runner::lang_iface::summary_from_witness_statuses(
            planned,
            witness,
            |selector| report_string_for_logical_string(&report_ids, selector),
            false,
        );
    }
    let report_ids =
        crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
            &request.repo_root,
            &[],
        )
        .unwrap_or_default();
    crate::test_runner::lang_iface::summary_from_witness_statuses(
        planned,
        witness,
        |selector| report_string_for_logical_string(&report_ids, selector),
        false,
    )
}

impl LanguageRuntime for RustRuntime {
    fn list(&self, request: &EnsureRequest) -> Result<Listing, String> {
        let identity =
            current_rust_coverage_batch_identity(&request.repo_root, &request.extras.rust)?;
        let (req, tools) = resolved_rust_batch_request_parts(&request.repo_root, &[])?;
        let record_identity = kiss::rust_llvm_cov_runner::rust_record_identity(&req, &tools)
            .map_err(|err| format!("error: kiss: rust record identity: {err}"))?;
        Ok(Listing {
            ids: request.planned.rust.clone(),
            identity: rust_identity_digest_from_batch(&identity),
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
        self.current_deps
            .borrow_mut()
            .get_or_insert_with(|| {
                kiss::rust_llvm_cov_runner::RustRecordDeps::new(&request.repo_root)
            })
            .current(row)
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
    miss_set: &[String],
    on_result: &mut dyn FnMut(SelectorExecutionRecord),
) -> Result<OutcomeBatch, String> {
    if miss_set.is_empty() {
        return Ok(OutcomeBatch::default());
    }
    let summary = crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_selectors_streaming(
        &request.repo_root,
        miss_set,
        crate::test_runner::rust_llvm_cov::RustCoverageRunOptions {
            extra: &request.extras.rust,
            force_rerun: request.force,
            force_rerun_selectors: &request.force_selectors,
            jobs: request.jobs,
            population_publication_selectors: rust_population_publication_selectors(
                request.mode,
                &request.planned.rust,
            ),
            coverage_output_mode: kiss::rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
            gate: request.gate.clone(),
        },
        on_result,
    )?;
    Ok(OutcomeBatch {
        summary,
        selectors: miss_set.to_vec(),
    })
}

pub(super) fn rust_accepted_summary(
    request: &EnsureRequest,
    planned: &[String],
    witness: &ExecutionWitness,
) -> Result<SelectorExecutionSummary, String> {
    let planned = rust_witness_overlap(planned, witness);
    let mut summary = rust_summary_from_witness_statuses(request, &planned, witness);
    if population_repair::repair_stale_population_on_all_mode_accept(request, &planned)? {
        summary.rust_derived_repair = true;
    }
    Ok(summary)
}

#[cfg(test)]
mod publication_selector_tests {
    use super::rust_population_publication_selectors;
    use crate::test_runner::lang_iface::AcceptMode;

    #[test]
    fn subset_does_not_publish_a_selective_miss_set_as_population() {
        let planned = vec!["a".into(), "b".into()];
        assert_eq!(
            rust_population_publication_selectors(AcceptMode::Subset, &planned),
            None
        );
    }

    #[test]
    fn all_mode_publishes_the_planned_universe() {
        let planned = vec!["a".into(), "b".into()];
        assert_eq!(
            rust_population_publication_selectors(AcceptMode::All, &planned),
            Some(planned.clone())
        );
    }
}

impl crate::test_runner::coverage_decision::SupportedLanguage for RustRuntime {
    fn language(&self) -> Language {
        Language::Rust
    }
}

#[cfg(test)]
#[path = "population_repair_test.rs"]
mod population_repair_tests;
