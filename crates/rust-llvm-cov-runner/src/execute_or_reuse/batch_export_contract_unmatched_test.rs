pub(super) fn assert_unmatched_selector(batch: &crate::RustCoverageBatchResult, debug: &str) {
    assert_eq!(batch.counters.unmatched_selectors, 1, "{debug}");
    assert!(
        batch.completed.is_empty(),
        "unmatched selective selectors must not be reported as completed PASS\n{debug}"
    );
    assert!(
        matches!(
            &batch.batch_error,
            Some(crate::RustLlvmCovError::InvalidRequest(message))
                if message.contains("did not execute 1 requested Rust selector")
        ),
        "unmatched selective selectors must fail the batch\n{debug}"
    );
}

pub(super) fn assert_mixed_matched_unmatched(batch: &crate::RustCoverageBatchResult, debug: &str) {
    assert_eq!(batch.counters.unmatched_selectors, 1, "{debug}");
    assert!(
        batch.completed.is_empty()
            || batch
                .batch_error
                .as_ref()
                .is_some_and(|err| matches!(err, crate::RustLlvmCovError::InvalidRequest(_))),
        "mixed matched/unmatched must not cache unmatched as PASS\n{debug}"
    );
}

pub(super) fn assert_exact_prefix_zero_instances(
    batch: &crate::RustCoverageBatchResult,
    selectors: &[String],
    debug: &str,
) {
    assert_eq!(
        batch.counters.unmatched_selectors,
        selectors.len(),
        "{debug}"
    );
    assert!(
        batch.completed.is_empty(),
        "exact-prefix with zero instances must not report PASS outcomes\n{debug}"
    );
    assert!(
        matches!(
            &batch.batch_error,
            Some(crate::RustLlvmCovError::InvalidRequest(_))
        ),
        "exact-prefix with zero instances must fail the batch\n{debug}"
    );
}
