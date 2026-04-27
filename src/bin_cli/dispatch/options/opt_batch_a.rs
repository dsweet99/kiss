use std::path::PathBuf;

use kiss::GateConfig;
use kiss::Language;

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
    pub cfg: &'a TriConfig<'a>,
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
