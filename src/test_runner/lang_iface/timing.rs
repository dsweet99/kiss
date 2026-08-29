pub(crate) fn session_timing_context_digest(jobs: usize) -> String {
    format!(
        "{}:{}:{}:{jobs}",
        kiss::rust_llvm_cov_runner::TIMING_CONTEXT_SCHEMA_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

pub(crate) fn timing_context_is_comparable(stored: &str, current: &str) -> bool {
    !stored.is_empty() && stored == current
}
