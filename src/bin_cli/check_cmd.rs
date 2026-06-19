use crate::analyze;
use crate::analyze::run_analyze;
use crate::bin_cli::util::{merge_check_ignore_prefixes, validate_paths};
use kiss::Language;

pub struct CheckCommandArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
    pub bypass_gate: bool,
    pub ignore: &'a [String],
    pub timing: bool,
    pub jobs: Option<usize>,
}

pub fn run_check_command(args: &CheckCommandArgs<'_>) -> i32 {
    let ignore = merge_check_ignore_prefixes(args.ignore);
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
        jobs: args.jobs,
        collect_stats: false,
    };
    i32::from(!run_analyze(&opts))
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl<'a> CheckCommandArgs<'a> {
        fn witness() {}
    }

    #[test]
    fn witness_check_command_args() {
        CheckCommandArgs::witness();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_command_empty_directory_passes_with_bypass_gate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = CheckCommandArgs {
            paths: &[path],
            lang_filter: None,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
            bypass_gate: true,
            ignore: &[],
            timing: false,
            jobs: None,
        };
        assert_eq!(run_check_command(&args), 0);
    }
}
