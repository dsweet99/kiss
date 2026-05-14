use kiss::Language;
use kiss::TestSectionConfig;

use super::TriConfig;

use crate::test_git::TestChangeMode;

pub(crate) struct ShrinkDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub target: Option<String>,
    pub paths: Vec<String>,
    pub ignore: Vec<String>,
    pub cfg: &'a TriConfig<'a>,
}

pub(crate) struct TestDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub mode: TestChangeMode,
    pub main_branch: Option<String>,
    pub base_branch: Option<String>,
    pub dry_run: bool,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub test_cfg: &'a TestSectionConfig,
}
