//! Pytest execution boundary for tools that need per-test outcomes.
//!
//! The cold subprocess runner is intentionally small. A later forkserver runner
//! can implement the same trait without changing coverage or cache callers.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

mod runner;
mod types;

#[cfg(test)]
mod tests;

pub use runner::{PytestRunner, SubprocessPytestRunner, subprocess_pytest_runner};
pub use types::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, RequestedArtifact, TestStatus,
};
