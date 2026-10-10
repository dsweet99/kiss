#![cfg_attr(not(test), allow(dead_code))]
use kiss::Language;

use super::records_witness::rust_witness_identity;
use crate::test_runner::lang_iface::{EnsureRequest, LanguageRuntime, Listing, OutcomeBatch};
use crate::test_runner::runners::SelectorExecutionRecord;

pub(crate) struct RustRuntime;

impl LanguageRuntime for RustRuntime {
    fn list(&self, request: &EnsureRequest) -> Result<Listing, String> {
        let record_identity =
            super::nextest::record_identity(&request.repo_root, &request.extras.rust)?;
        Ok(Listing {
            ids: request.planned.rust.clone(),
            identity: rust_witness_identity(&record_identity),
        })
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

impl crate::test_runner::test_selection::SupportedLanguage for RustRuntime {
    fn language(&self) -> Language {
        Language::Rust
    }
}
