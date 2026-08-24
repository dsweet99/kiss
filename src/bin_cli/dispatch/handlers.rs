use crate::analyze;
use crate::analyze::DryRunParams;
use crate::bin_cli::stats::{RunStatsArgs, run_stats};
use crate::bin_cli::test_cmd::run_test_command;
use crate::bin_cli::util;
use crate::bin_cli::{check_cmd, cov_cmd};
use crate::rules::run_rules;
use crate::viz::{VizCoarsen, run_viz};

use super::options::{
    CheckDispatchOptions, CovDispatchOptions, DryDispatchOptions, MvDispatchOptions,
    RulesDispatchOptions, StatsDispatchOptions, TestDispatchOptions, VizDispatchOptions,
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
        config: o.config.as_deref(),
        language_tables: o.cfg.language_tables,
    };
    check_cmd::run_check_command(&args)
}

pub(in crate::bin_cli::dispatch) fn dispatch_cov(o: CovDispatchOptions<'_>) -> i32 {
    let pytest_args = o.test_cfg.pytest_plugin_cli_args();
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
        allow_refresh: true,
        pytest_args: &pytest_args,
        language_tables: o.cfg.language_tables,
    };
    cov_cmd::run_cov_command(&args)
}

pub(in crate::bin_cli::dispatch) fn dispatch_stats(o: StatsDispatchOptions) -> i32 {
    let ignore = util::merge_check_ignore_prefixes(&o.ignore);
    run_stats(RunStatsArgs {
        paths: &o.paths,
        lang_filter: o.lang,
        ignore: &ignore,
        all: o.all,
        table: o.table,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
        language_tables: o.cfg.language_tables,
        config: o.config.as_deref(),
    })
}

pub(in crate::bin_cli::dispatch) fn dispatch_dry(o: DryDispatchOptions) -> i32 {
    let ignore = util::merge_check_ignore_prefixes(&o.ignore);
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
        language_tables: o.language_tables,
    };
    analyze::run_dry(&params)
}

pub(in crate::bin_cli::dispatch) fn dispatch_rules(o: RulesDispatchOptions<'_>) -> i32 {
    run_rules(o.cfg.py, o.cfg.rs, o.cfg.gate, o.lang);
    0
}

pub(in crate::bin_cli::dispatch) fn dispatch_viz(o: VizDispatchOptions) -> i32 {
    let ignore = util::merge_check_ignore_prefixes(&o.ignore);
    util::validate_paths(&o.paths);
    let coarsen = o
        .num_nodes
        .map_or(VizCoarsen::Zoom(o.zoom), VizCoarsen::NumNodes);
    if let Err(e) = run_viz(
        &o.out,
        &o.paths,
        o.lang,
        &ignore,
        coarsen,
        o.language_tables,
    ) {
        eprint_viz_error(&e);
        return 1;
    }
    0
}

fn eprint_viz_error(e: &std::io::Error) {
    let msg = e.to_string();
    if msg.starts_with("Error:") {
        eprintln!("{msg}");
    } else {
        eprintln!("Error: {msg}");
    }
}

pub(in crate::bin_cli::dispatch) fn dispatch_test(o: TestDispatchOptions<'_>) -> i32 {
    let cli_ignore = o.ignore.clone();
    let ignore = o.test_cfg.merged_ignore(&o.ignore);
    run_test_command(crate::bin_cli::test_cmd::TestCommandArgs {
        invocation: o.invocation,
        main_branch: o.main_branch.as_deref(),
        base_branch: o.base_branch.as_deref(),
        dry_run: o.dry_run,
        force: o.force,
        force_bad: o.force_bad,
        metrics: o.metrics,
        coverage_all: o.coverage_all,
        watch: o.watch,
        jobs: o.jobs.unwrap_or(o.test_cfg.num_jobs),
        jobs_cli: o.jobs,
        ignore: &ignore,
        cli_ignore: &cli_ignore,
        extra: &o.extra,
        lang_filter: o.lang,
        test_cfg: o.test_cfg,
        py_config: o.cfg.py,
        rs_config: o.cfg.rs,
        gate_config: o.cfg.gate,
        reload_kissconfig: o.reload_kissconfig,
        config_path: o.config_path,
        language_tables: o.cfg.language_tables,
    })
}

pub(in crate::bin_cli::dispatch) fn dispatch_mv(o: MvDispatchOptions) -> i32 {
    let ignore = util::merge_check_ignore_prefixes(&o.ignore);
    let opts = kiss::symbol_mv::MvOptions {
        query: o.query,
        new_name: o.new_name,
        paths: o.paths,
        to: o.to,
        dry_run: o.mv_flags.dry_run,
        json: o.mv_flags.json,
        lang_filter: o.lang,
        ignore,
        language_tables: o.language_tables,
    };
    kiss::symbol_mv::run_mv_command(opts)
}

#[cfg(test)]
mod viz_error_tests {
    use super::*;
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    #[test]
    fn eprint_viz_error_both_prefix_branches() {
        eprint_viz_error(&Error::new(
            ErrorKind::InvalidInput,
            "Error: found rust files",
        ));
        eprint_viz_error(&Error::new(
            ErrorKind::InvalidInput,
            "No source files found.",
        ));
    }

    #[test]
    fn dispatch_viz_prints_reclamp_without_double_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.py"), "x = 1\n").unwrap();
        let code = dispatch_viz(VizDispatchOptions {
            lang: None,
            out: tmp.path().join("g.mmd"),
            paths: vec![tmp.path().to_string_lossy().into_owned()],
            zoom: 1.0,
            num_nodes: None,
            ignore: vec![],
            language_tables: kiss::LanguageTablesPresent::none(),
        });
        assert_eq!(code, 1);
        let empty = tempfile::tempdir().unwrap();
        let empty_code = dispatch_viz(VizDispatchOptions {
            lang: None,
            out: PathBuf::from("g.mmd"),
            paths: vec![empty.path().to_string_lossy().into_owned()],
            zoom: 1.0,
            num_nodes: None,
            ignore: vec![],
            language_tables: kiss::LanguageTablesPresent::both(),
        });
        assert_eq!(empty_code, 1);
    }
}
