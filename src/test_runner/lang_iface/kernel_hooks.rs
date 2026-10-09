#![cfg_attr(not(test), allow(dead_code))]
use super::runtime::EnsureRequest;
use super::witness::ExecutionWitness;
use super::witness_summary::summary_from_witness_statuses;
use crate::test_runner::runners::SelectorExecutionSummary;

pub(crate) trait KernelHooks: Sync {
    fn report_labels(
        &self,
        repo_root: &std::path::Path,
        selectors: &[String],
    ) -> std::collections::BTreeMap<String, String> {
        let _ = (repo_root, selectors);
        std::collections::BTreeMap::new()
    }

    fn is_test_source(&self, path: &std::path::Path) -> bool {
        let _ = path;
        false
    }

    fn validate_extra_args(&self, extras: &[String]) -> Result<(), String> {
        let _ = extras;
        Ok(())
    }

    fn validate_explicit_targets(
        &self,
        repo_root: &std::path::Path,
        files: &[std::path::PathBuf],
        direct: &std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        let _ = (repo_root, files, direct);
        Ok(())
    }

    fn extras_block_cold_population(&self, extras: &[String]) -> bool {
        let _ = extras;
        false
    }

    fn cached_witness_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        let _ = request;
        summary_from_witness_statuses(planned, witness, |selector| selector.to_string(), false)
    }
}
