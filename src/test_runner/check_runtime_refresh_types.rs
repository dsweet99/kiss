
use std::path::Path;

use kiss::GateConfig;

use super::{
    CoverageRefreshError, CoverageRefreshStats, LanguageRefreshStats, ensure_python_runtime_coverage,
    ensure_rust_runtime_coverage,
};

pub(crate) trait CoverageRuntimeRefresh {
    fn language(&self) -> kiss::Language;
    fn ensure(
        &self,
        repo_root: &Path,
        ignore: &[String],
        jobs: usize,
        pytest_args: &[String],
        gate: &GateConfig,
    ) -> Result<CoverageRefreshStats, CoverageRefreshError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PythonRuntimeRefresh;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RustRuntimeRefresh;

impl CoverageRuntimeRefresh for PythonRuntimeRefresh {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }

    fn ensure(
        &self,
        repo_root: &Path,
        ignore: &[String],
        jobs: usize,
        pytest_args: &[String],
        gate: &GateConfig,
    ) -> Result<CoverageRefreshStats, CoverageRefreshError> {
        ensure_python_runtime_coverage(repo_root, ignore, jobs, pytest_args, gate)
    }
}

impl CoverageRuntimeRefresh for RustRuntimeRefresh {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn ensure(
        &self,
        repo_root: &Path,
        ignore: &[String],
        jobs: usize,
        _pytest_args: &[String],
        gate: &GateConfig,
    ) -> Result<CoverageRefreshStats, CoverageRefreshError> {
        ensure_rust_runtime_coverage(repo_root, ignore, jobs, gate)
    }
}

impl CoverageRefreshStats {
    pub(crate) fn for_rust(stats: LanguageRefreshStats) -> Self {
        Self {
            by_language: crate::test_runner::language_keyed::LanguageKeyed {
                python: LanguageRefreshStats::default(),
                rust: stats,
            },
        }
    }
}
