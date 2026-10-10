#![cfg_attr(not(test), allow(dead_code))]

use kiss::rpytest_runner::TestStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcceptMode {
    All,
    Subset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WitnessStatus {
    Passed,
    Failed,
    TimedOut,
    Unresolved,
}

impl WitnessStatus {
    pub(crate) fn from_test_status(status: TestStatus) -> Self {
        match status {
            TestStatus::Passed => Self::Passed,
            TestStatus::Failed => Self::Failed,
            TestStatus::TimedOut => Self::TimedOut,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Unresolved => "unresolved",
        }
    }

    #[cfg(test)]
    pub(crate) fn parse(raw: &str) -> Self {
        match raw {
            "passed" | "Passed" | "PASS" => Self::Passed,
            "failed" | "Failed" | "FAIL" => Self::Failed,
            "timed_out" | "TimedOut" | "TIMEOUT" => Self::TimedOut,
            _ => Self::Unresolved,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExecutionWitness {
    pub(crate) language: kiss::Language,
    pub(crate) identity_digest: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<Option<u64>>,
    pub(crate) complete: bool,
    pub(crate) generation_id: String,
    pub(crate) raw_statuses: Vec<WitnessStatus>,
}
