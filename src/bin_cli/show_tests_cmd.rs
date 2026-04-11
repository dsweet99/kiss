use crate::bin_cli::util::{normalize_ignore_prefixes, validate_paths};
use crate::show_tests;
use kiss::Language;

pub struct RunShowTestsCmdArgs<'a> {
    pub universe: &'a str,
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
    pub show_untested: bool,
}

pub fn run_show_tests(args: RunShowTestsCmdArgs<'_>) -> i32 {
    let ignore = normalize_ignore_prefixes(args.ignore);
    validate_paths(args.paths);
    show_tests::run_show_tests_to(show_tests::args::RunShowTestsArgs {
        out: &mut std::io::stdout(),
        universe: args.universe,
        paths: args.paths,
        lang_filter: args.lang_filter,
        ignore: &ignore,
        show_untested: args.show_untested,
    })
}
