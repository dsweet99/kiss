use std::path::PathBuf;

use kiss::Language;

pub(crate) struct MvDispatchOptions {
    pub lang: Option<Language>,
    pub query: String,
    pub new_name: String,
    pub paths: Vec<String>,
    pub to: Option<PathBuf>,
    pub mv_flags: MvOutputFlags,
    pub ignore: Vec<String>,
}

pub(crate) struct MvOutputFlags {
    pub dry_run: bool,
    pub json: bool,
}
