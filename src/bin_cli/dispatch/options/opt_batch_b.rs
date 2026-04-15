use std::path::PathBuf;

use kiss::Language;

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
}

pub(crate) struct VizDispatchOptions {
    pub lang: Option<Language>,
    pub out: PathBuf,
    pub paths: Vec<String>,
    pub zoom: f64,
    pub ignore: Vec<String>,
}
