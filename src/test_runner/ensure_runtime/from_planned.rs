//! Arguments for `ensure_request_from_planned` (keeps the call site under arg thresholds).

use std::path::PathBuf;

use kiss::{GateConfig, Language};

use crate::test_runner::lang_iface::AcceptMode;
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::PlannedSelectors;

pub(crate) struct EnsureFromPlanned<'a> {
    pub(crate) planned: &'a PlannedSelectors,
    pub(crate) mode: AcceptMode,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) force: bool,
    /// Prior-failure selectors that must invalidate reuse without batch-wide force.
    pub(crate) force_selectors: Vec<String>,
    pub(crate) jobs: usize,
    /// Per-language CLI extras (same abstract slot as `EnsureRequest.extras`).
    pub(crate) extras: LanguageKeyed<&'a [String]>,
    pub(crate) repo_root_override: Option<PathBuf>,
    pub(crate) gate: GateConfig,
}
