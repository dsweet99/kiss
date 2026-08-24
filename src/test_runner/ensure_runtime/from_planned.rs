use std::path::PathBuf;

use kiss::{GateConfig, Language};

use crate::test_runner::PlannedSelectors;
use crate::test_runner::lang_iface::AcceptMode;
use crate::test_runner::language_keyed::LanguageKeyed;

pub(crate) struct EnsureFromPlanned<'a> {
    pub(crate) planned: &'a PlannedSelectors,
    pub(crate) mode: AcceptMode,
    pub(crate) lang_filter: Option<Language>,
    pub(crate) force: bool,
    pub(crate) force_selectors: Vec<String>,
    pub(crate) jobs: usize,
    pub(crate) extras: LanguageKeyed<&'a [String]>,
    pub(crate) repo_root_override: Option<PathBuf>,
    pub(crate) gate: GateConfig,
}
