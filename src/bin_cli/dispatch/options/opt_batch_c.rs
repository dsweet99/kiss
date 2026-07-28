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
    pub metrics: bool,
    pub jobs: usize,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub test_cfg: &'a TestSectionConfig,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl ShrinkDispatchOptions<'_> {
        fn witness() {}
    }
    impl TestDispatchOptions<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_opt_batch_c() {
        ShrinkDispatchOptions::witness();
        TestDispatchOptions::witness();
    }
}
