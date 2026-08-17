use kiss::Language;
use kiss::TestSectionConfig;

use super::TriConfig;

use crate::bin_cli::args::TestInvocation;

pub(crate) struct ShrinkDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub target: Option<String>,
    pub paths: Vec<String>,
    pub ignore: Vec<String>,
    pub cfg: &'a TriConfig<'a>,
}

pub(crate) struct TestDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub invocation: TestInvocation,
    pub main_branch: Option<String>,
    pub base_branch: Option<String>,
    pub dry_run: bool,
    pub force: bool,
    pub force_bad: bool,
    pub metrics: bool,
    pub coverage_all: bool,
    pub watch: bool,
    pub jobs: Option<usize>,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub test_cfg: &'a TestSectionConfig,
    pub cfg: &'a TriConfig<'a>,
    pub reload_kissconfig: bool,
    pub config_path: Option<&'a std::path::PathBuf>,
}

#[cfg(test)]
#[path = "opt_batch_c_test.rs"]
mod coverage_witness;

