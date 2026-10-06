const TIMING_CONTEXT_SCHEMA_VERSION: &str = "kiss-timing-context-v1";

pub(crate) fn session_timing_context_digest(_jobs: usize) -> String {
    format!(
        "{}:{}:{}",
        TIMING_CONTEXT_SCHEMA_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

pub(crate) fn timing_context_is_comparable(stored: &str, current: &str) -> bool {
    !stored.is_empty() && stored == current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_context_is_independent_of_job_split() {
        assert_eq!(
            session_timing_context_digest(4),
            session_timing_context_digest(8)
        );
    }
}
