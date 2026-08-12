//! Arguments for `ensure_request_from_planned` (keeps the call site under arg thresholds).

use std::path::PathBuf;

use kiss::{GateConfig, Language};

use crate::test_runner::lang_iface::AcceptMode;
use crate::test_runner::PlannedSelectors;

pub(crate) struct EnsureFromPlanned<'a> {
    pub(crate) planned: &'a PlannedSelectors,
    pub(crate) mode: AcceptMode,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) force: bool,
    pub(crate) jobs: usize,
    pub(crate) python_extra: &'a [String],
    pub(crate) rust_extra: &'a [String],
    pub(crate) repo_root_override: Option<PathBuf>,
    pub(crate) gate: GateConfig,
}
