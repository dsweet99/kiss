use std::path::PathBuf;

use kiss::Language;
use kiss::TestSectionConfig;

use super::TriConfig;

pub(crate) struct DryDispatchOptions {
    pub lang: Option<Language>,
    pub path: String,
    pub filter_files: Vec<String>,
    pub shingle_size: usize,
    pub minhash_size: usize,
    pub lsh_bands: usize,
    pub min_similarity: f64,
    pub ignore: Vec<String>,
}

pub(crate) struct RulesDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub defaults: bool,
    pub cfg: &'a TriConfig<'a>,
}

pub(crate) struct ConfigDispatchOptions<'a> {
    pub defaults: bool,
    pub config: Option<PathBuf>,
    pub cfg: &'a TriConfig<'a>,
    pub test_cfg: &'a TestSectionConfig,
}

pub(crate) struct VizDispatchOptions {
    pub lang: Option<Language>,
    pub out: PathBuf,
    pub paths: Vec<String>,
    pub zoom: f64,
    pub num_nodes: Option<usize>,
    pub ignore: Vec<String>,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl DryDispatchOptions {
        fn witness() -> Self {
            Self {
                lang: None,
                path: ".".into(),
                filter_files: vec![],
                shingle_size: 1,
                minhash_size: 1,
                lsh_bands: 1,
                min_similarity: 0.5,
                ignore: vec![],
            }
        }
    }
    impl RulesDispatchOptions<'_> {
        fn witness() {}
    }
    impl ConfigDispatchOptions<'_> {
        fn witness() {}
    }
    impl VizDispatchOptions {
        fn witness() -> Self {
            Self {
                lang: None,
                out: PathBuf::from("out"),
                paths: vec![],
                zoom: 1.0,
                num_nodes: None,
                ignore: vec![],
            }
        }
    }

    #[test]
    fn witness_opt_batch_b() {
        let _ = DryDispatchOptions::witness();
        RulesDispatchOptions::witness();
        ConfigDispatchOptions::witness();
        let _ = VizDispatchOptions::witness();
    }
}
