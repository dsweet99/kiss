use kiss::Language;

use super::TriConfig;

pub(crate) struct ShrinkDispatchOptions<'a> {
    pub lang: Option<Language>,
    pub target: Option<String>,
    pub paths: Vec<String>,
    pub ignore: Vec<String>,
    pub cfg: &'a TriConfig<'a>,
}

pub(crate) struct ShowTestsDispatchOptions {
    pub lang: Option<Language>,
    pub paths: Vec<String>,
    pub untested: bool,
    pub ignore: Vec<String>,
}
