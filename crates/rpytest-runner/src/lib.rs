//! Pytest execution boundary for tools that need per-test outcomes.
//!
//! The cold subprocess and forkserver runners share one outcome contract so
//! coverage and cache callers do not need to own pytest process details.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod forkserver;
mod runner;
mod types;

#[cfg(test)]
mod forkserver_test;
#[cfg(test)]
mod runner_test;
#[cfg(test)]
mod tests;

pub use forkserver::{ForkserverPytestRunner, forkserver_pytest_runner};
pub use runner::{PytestRunner, SubprocessPytestRunner, subprocess_pytest_runner};
pub use types::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, RequestedArtifact, TestStatus,
};
