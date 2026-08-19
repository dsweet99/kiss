use std::collections::BTreeMap;
use std::path::Path;

use rust_llvm_cov_runner::CoverageOutputMode;

fn timeout_rules_need_report_ids(rules: &[(String, f64)]) -> bool {
    rules.iter().any(|(pattern, _)| pattern != "*")
}

fn timeout_millis_from_limit(secs: f64) -> u64 {
    if secs.is_finite() && secs > 0.0 {
        (secs * 1000.0).round().clamp(1.0, u64::MAX as f64) as u64
    } else {
        0
    }
}

pub(super) fn selector_timeout_millis_for_batch(
    repo_root: &Path,
    selectors: &[String],
    coverage_output_mode: &CoverageOutputMode,
    gate: &kiss::GateConfig,
) -> Result<BTreeMap<String, u64>, String> {



    if matches!(coverage_output_mode, CoverageOutputMode::CheckAggregate { .. })
        && !timeout_rules_need_report_ids(&gate.max_unit_test_seconds)
    {
        return Ok(selectors
            .iter()
            .map(|selector| {
                let secs = kiss::limit_for_selector(&gate.max_unit_test_seconds, selector);
                (selector.clone(), timeout_millis_from_limit(secs))
            })
            .collect());
    }


    let report_ids = crate::test_runner::runners::rust_report_ids_for_selectors(repo_root, selectors)?;
    selectors
        .iter()
        .map(|selector| {
            let for_limit = crate::test_runner::runners::require_kiss_test_report_id(
                &report_ids,
                selector,
            )?;
            let secs = kiss::limit_for_selector(&gate.max_unit_test_seconds, &for_limit);
            Ok((selector.clone(), timeout_millis_from_limit(secs)))
        })
        .collect()
}
