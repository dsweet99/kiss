//! Pytest execution boundary for tools that need per-test outcomes.
//!
//! The cold subprocess and forkserver runners share one outcome contract so
//! coverage and cache callers do not need to own pytest process details.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod collector;
mod forkserver;
mod forkserver_controller;
mod runner;
mod types;

#[cfg(test)]
#[path = "collector_test.rs"]
mod collector_test;

#[cfg(test)]
mod bounded_concurrency_test_support;
#[cfg(test)]
#[path = "bounded_concurrency_test_support_test.rs"]
mod bounded_concurrency_test_support_test;
#[cfg(test)]
mod forkserver_test;
#[cfg(test)]
#[path = "forkserver_timeout_test.rs"]
mod forkserver_timeout_test;
#[cfg(test)]
mod runner_test;
#[cfg(test)]
mod tests;

pub use collector::{
    PytestCollectError, PytestCollectOutcome, PytestCollectRequest, SubprocessPytestCollector,
    collect_pytest_nodeids, subprocess_pytest_collector,
};
pub use forkserver::{ForkserverPytestRunner, forkserver_pytest_runner};
pub use runner::{PytestRunner, SubprocessPytestRunner, subprocess_pytest_runner};
pub use types::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, RequestedArtifact, TestStatus,
};
