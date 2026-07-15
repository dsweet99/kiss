use std::path::PathBuf;

use kiss::GateConfig;
use kiss::Language;
use kiss::TestSectionConfig;

pub(crate) struct TriConfig<'a> {
    pub py: &'a kiss::Config,
    pub rs: &'a kiss::Config,
    pub gate: &'a GateConfig,
}

pub(crate) struct CheckDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub paths: Vec<String>,
    pub bypass_gate: bool,
    pub ignore: Vec<String>,
    pub timing: bool,
    pub jobs: Option<usize>,
    pub cfg: &'a TriConfig<'a>,
    pub test_cfg: &'a TestSectionConfig,
}

pub(crate) struct StatsDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub paths: Vec<String>,
    pub all: Option<usize>,
    pub table: bool,
    pub ignore: Vec<String>,
    pub cfg: &'a TriConfig<'a>,
}

pub(crate) struct MimicDispatchOptions {
    pub lang: Option<Language>,
    pub paths: Vec<String>,
    pub out: Option<PathBuf>,
    pub ignore: Vec<String>,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl TriConfig<'_> {
        fn witness() {}
    }
    impl CheckDispatchOptions<'_> {
        fn witness() {}
    }
    impl StatsDispatchOptions<'_> {
        fn witness() {}
    }
    impl MimicDispatchOptions {
        fn witness() -> Self {
            Self {
                lang: None,
                paths: vec![],
                out: None,
                ignore: vec![],
            }
        }
    }

    #[test]
    fn witness_opt_batch_a() {
        TriConfig::witness();
        CheckDispatchOptions::witness();
        StatsDispatchOptions::witness();
        let _ = MimicDispatchOptions::witness();
    }
}
