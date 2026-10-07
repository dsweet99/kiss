use kiss::Language;
use kiss::TestSectionConfig;

use super::TriConfig;

use crate::bin_cli::args::TestInvocation;

pub(crate) struct TestDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub invocation: TestInvocation,
    pub main_branch: Option<String>,
    pub base_branch: Option<String>,
    pub dry_run: bool,
    pub retry_bad: bool,
    pub metrics: bool,
    pub jobs: Option<usize>,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub test_cfg: &'a TestSectionConfig,
    pub cfg: &'a TriConfig<'a>,
}

#[cfg(test)]
#[path = "opt_batch_c_test.rs"]
mod touch_witness;
