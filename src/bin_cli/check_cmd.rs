use crate::analyze;
use crate::analyze::run_analyze;
use crate::bin_cli::util::validate_paths;
use kiss::normalize_ignore_prefixes;
use kiss::Language;

/// Directory or filename prefixes excluded from `kiss check` by default.
/// Intentionally-violating fixtures live under `tests/fake_python/` and
/// `tests/fake_rust/`; they are analyzed directly in integration tests but
/// must not fail the repo's own quality gate.
const DEFAULT_CHECK_IGNORE_PREFIXES: &[&str] = &["fake_"];

pub struct CheckCommandArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
    pub bypass_gate: bool,
    pub ignore: &'a [String],
    pub timing: bool,
}

pub fn run_check_command(args: &CheckCommandArgs<'_>) -> i32 {
    let mut ignore: Vec<String> = DEFAULT_CHECK_IGNORE_PREFIXES
        .iter()
        .map(|prefix| (*prefix).to_string())
        .collect();
    ignore.extend(args.ignore.iter().cloned());
    let ignore = normalize_ignore_prefixes(&ignore);
    validate_paths(args.paths);
    let universe = &args.paths[0];
    let focus = if args.paths.len() > 1 {
        &args.paths[1..]
    } else {
        args.paths
    };
    let opts = analyze::AnalyzeOptions {
        universe,
        focus_paths: focus,
        py_config: args.py_config,
        rs_config: args.rs_config,
        lang_filter: args.lang_filter,
        bypass_gate: args.bypass_gate,
        gate_config: args.gate_config,
        ignore_prefixes: &ignore,
        show_timing: args.timing,
        suppress_final_status: false,
    };
    i32::from(!run_analyze(&opts))
}
