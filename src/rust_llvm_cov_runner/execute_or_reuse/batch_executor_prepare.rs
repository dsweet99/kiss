use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::{RustCovCacheStatus, RustLlvmCovOutcome};

pub(crate) fn selector_timeout_is_ban(req: &RustCoverageBatchRequest, selector: &str) -> bool {
    req.selector_timeout_millis.get(selector) == Some(&0)
}

pub(crate) fn banned_timeout_outcome(selector: &str) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status: TestStatus::TimedOut,
        exit_code: Some(124),
        duration: std::time::Duration::ZERO,
        coverage: Default::default(),
        test_binary_ids: Vec::new(),
        cache_status: RustCovCacheStatus::FreshUnstored,
        stdout: None,
        stderr: None,
    }
}
