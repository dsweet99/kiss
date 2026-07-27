use std::path::Path;

use crate::analyze;
use crate::analyze::DryRunParams;
use crate::bin_cli::{check_cmd, cov_cmd};
use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::shrink::{RunShrinkArgs, ShrinkFullContext, run_shrink};
use crate::bin_cli::stats::{RunStatsArgs, run_stats};
use crate::bin_cli::test_cmd::run_test_command;
use crate::bin_cli::util;
use crate::rules::{run_config, run_rules};
use crate::viz::{VizCoarsen, run_viz};
use kiss::{Language, normalize_ignore_prefixes};

use super::options::{
    CheckDispatchOptions, ConfigDispatchOptions, CovDispatchOptions, DryDispatchOptions,
    MimicDispatchOptions, MvDispatchOptions, RulesDispatchOptions, ShrinkDispatchOptions,
    StatsDispatchOptions, TestDispatchOptions, VizDispatchOptions,
};

pub(in crate::bin_cli::dispatch) fn dispatch_check(o: CheckDispatchOptions<'_>) -> i32 {
    let args = check_cmd::CheckCommandArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
        ignore: &o.ignore,
        timing: o.timing,
    };
    check_cmd::run_check_command(&args)
}

pub(in crate::bin_cli::dispatch) fn dispatch_cov(o: CovDispatchOptions<'_>) -> i32 {
    let args = cov_cmd::CovCommandArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
        bypass_gate: o.bypass_gate,
        ignore: &o.ignore,
        timing: o.timing,
        jobs: o.jobs.unwrap_or(o.test_cfg.num_jobs),
    };
    cov_cmd::run_cov_command(&args)
}

pub(in crate::bin_cli::dispatch) fn dispatch_stats(o: StatsDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    run_stats(RunStatsArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        ignore: &ignore,
        all: o.all,
        table: o.table,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
    });
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_mimic(o: MimicDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    run_mimic(&o.paths, o.out.as_deref(), o.lang, &ignore)
}

pub(in crate::bin_cli::dispatch) fn dispatch_clamp(
    lang: Option<Language>,
    ignore: Vec<String>,
) -> i32 {
    let ignore = util::merge_check_ignore_prefixes(&ignore);
    run_mimic(
        &[".".to_string()],
        Some(Path::new(".kissconfig")),
        lang,
        &ignore,
    )
}

pub(in crate::bin_cli::dispatch) fn dispatch_dry(o: DryDispatchOptions) -> i32 {
    let ignore = normalize_ignore_prefixes(&o.ignore);
    if let Err(msg) = util::validate_min_similarity(o.min_similarity) {
        eprintln!("Error: {msg}");
        return 1;
    }
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
    util::validate_paths(&o.paths);
    let coarsen = o
        .num_nodes
        .map_or(VizCoarsen::Zoom(o.zoom), VizCoarsen::NumNodes);
    if let Err(e) = run_viz(&o.out, &o.paths, o.lang, &ignore, coarsen) {
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

pub(in crate::bin_cli::dispatch) fn dispatch_test(o: TestDispatchOptions<'_>) -> i32 {
    match o.action {
        crate::bin_cli::args::TestCommandAction::Run(mode) => {
            run_test_command(crate::bin_cli::test_cmd::TestCommandArgs {
                mode,
                main_branch: o.main_branch.as_deref(),
                base_branch: o.base_branch.as_deref(),
                dry_run: o.dry_run,
                force: o.force,
                metrics: o.metrics,
                jobs: o.jobs,
                ignore: &o.ignore,
                extra: &o.extra,
                lang_filter: o.lang,
                test_cfg: o.test_cfg,
            })
        }
        crate::bin_cli::args::TestCommandAction::ValidateSelection(mode) => {
            crate::bin_cli::test_cmd::run_validate_selection_command(
                crate::bin_cli::test_cmd::ValidateSelectionCommandArgs {
                    mode,
                    main_branch: o.main_branch.as_deref(),
                    base_branch: o.base_branch.as_deref(),
                    dry_run: o.dry_run,
                    jobs: o.jobs,
                    ignore: &o.ignore,
                    extra: &o.extra,
                    lang_filter: o.lang,
                    fixture: o.fixture.as_deref(),
                    test_cfg: o.test_cfg,
                },
            )
        }
        crate::bin_cli::args::TestCommandAction::Cov => 2,
    }
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
