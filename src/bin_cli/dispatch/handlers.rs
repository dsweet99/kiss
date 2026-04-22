use std::path::Path;

use crate::analyze;
use crate::analyze::DryRunParams;
use crate::bin_cli::check_cmd::{CheckCommandArgs, run_check_command};
use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::show_tests_cmd::{RunShowTestsCmdArgs, run_show_tests};
use crate::bin_cli::shrink::{RunShrinkArgs, ShrinkFullContext, run_shrink};
use crate::bin_cli::stats::{RunStatsArgs, run_stats};
use crate::bin_cli::util::{normalize_ignore_prefixes, validate_paths};
use crate::rules::{run_config, run_rules};
use crate::viz::run_viz;
use kiss::Language;

use super::options::{
    CheckDispatchOptions, ConfigDispatchOptions, DryDispatchOptions, MimicDispatchOptions,
    MvDispatchOptions, RulesDispatchOptions, ShowTestsDispatchOptions, ShrinkDispatchOptions,
    StatsDispatchOptions, VizDispatchOptions,
};

pub(in crate::bin_cli::dispatch) fn dispatch_check(o: CheckDispatchOptions<'_>) -> i32 {
    let args = CheckCommandArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
        bypass_gate: o.bypass_gate,
        ignore: &o.ignore,
        timing: o.timing,
    };
    run_check_command(&args)
}

pub(in crate::bin_cli::dispatch) fn dispatch_stats(o: StatsDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    run_stats(RunStatsArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        ignore: &ignore,
        all: o.all,
        table: o.table,
    });
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_mimic(o: MimicDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    run_mimic(&o.paths, o.out.as_deref(), o.lang, &ignore);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_clamp(
    lang: Option<Language>,
    ignore: Vec<String>,
) -> i32 {
    let ignore = normalize_ignore_prefixes(&ignore);
    run_mimic(
        &[".".to_string()],
        Some(Path::new(".kissconfig")),
        lang,
        &ignore,
    );
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_dry(o: DryDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    let config = kiss::DuplicationConfig {
        shingle_size: o.shingle_size,
        minhash_size: o.minhash_size,
        lsh_bands: o.lsh_bands,
        min_similarity: o.min_similarity,
    };
    let params = DryRunParams {
        path: o.path.as_str(),
        filter_files: &o.filter_files,
        config: &config,
        ignore_prefixes: &ignore,
        lang_filter: o.lang,
    };
    analyze::run_dry(&params);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_rules(o: RulesDispatchOptions<'_>) -> i32 {
    run_rules(o.cfg.py, o.cfg.rs, o.cfg.gate, o.lang, o.defaults);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_config(o: ConfigDispatchOptions<'_>) -> i32 {
    run_config(
        o.cfg.py,
        o.cfg.rs,
        o.cfg.gate,
        o.config.as_ref(),
        o.defaults,
    );
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_viz(o: VizDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    validate_paths(&o.paths);
    if let Err(e) = run_viz(&o.out, &o.paths, o.lang, &ignore, o.zoom) {
        eprintln!("Error: {e}");
        return 1;
    }
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_shrink(o: ShrinkDispatchOptions<'_>) -> i32 {
    let ctx = ShrinkFullContext {
        lang_filter: o.lang,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
    };
    run_shrink(RunShrinkArgs {
        target: o.target,
        paths: &o.paths,
        ignore: &o.ignore,
        ctx: &ctx,
    })
}

pub(in crate::bin_cli::dispatch) fn dispatch_show_tests(o: ShowTestsDispatchOptions) -> i32 {
    run_show_tests(RunShowTestsCmdArgs {
        universe: ".",
        paths: &o.paths,
        lang_filter: o.lang,
        ignore: &o.ignore,
        show_untested: o.untested,
    })
}

pub(in crate::bin_cli::dispatch) fn dispatch_mv(o: MvDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    let opts = kiss::symbol_mv::MvOptions {
        query: o.query,
        new_name: o.new_name,
        paths: o.paths,
        to: o.to,
        dry_run: o.mv_flags.dry_run,
        json: o.mv_flags.json,
        lang_filter: o.lang,
        ignore,
    };
    kiss::symbol_mv::run_mv_command(opts)
}
