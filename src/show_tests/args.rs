//! Argument bundles for `show_tests` (keeps public functions under `arguments_per_function` limits).

use kiss::Language;

use super::DefEntry;

/// Parameters for [`super::run_show_tests_to`](crate::show_tests::run_show_tests_to).
pub struct RunShowTestsArgs<'a> {
    pub out: &'a mut dyn std::io::Write,
    pub universe: &'a str,
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
    pub show_untested: bool,
}

pub struct EmitShowTestsArgs<'a, 'g> {
    pub out: &'a mut dyn std::io::Write,
    pub all_defs: &'a [DefEntry],
    pub show_untested: bool,
    pub py_graph: Option<&'g kiss::DependencyGraph>,
    pub rs_graph: Option<&'g kiss::DependencyGraph>,
}
