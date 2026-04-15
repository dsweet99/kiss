use crate::bin_cli::util::{normalize_ignore_prefixes, validate_paths};
use crate::layout;
use kiss::Language;
use std::path::Path;

pub struct LayoutCommandArgs<'a> {
    pub paths: &'a [String],
    pub out: Option<&'a Path>,
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
    pub project_name: Option<String>,
}

pub fn run_layout_command(args: LayoutCommandArgs<'_>) -> i32 {
    let ignore = normalize_ignore_prefixes(args.ignore);
    validate_paths(args.paths);
    let opts = layout::LayoutOptions {
        paths: args.paths,
        lang_filter: args.lang_filter,
        ignore_prefixes: &ignore,
        project_name: args.project_name,
    };
    if let Err(e) = layout::run_layout(&opts, args.out) {
        eprintln!("Error: {e}");
        return 1;
    }
    0
}
