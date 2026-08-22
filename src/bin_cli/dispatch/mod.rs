mod handlers;
mod options;

#[cfg(test)]
mod test_dispatch;
#[cfg(test)]
mod test_dispatch_b;

use crate::bin_cli::args::{Cli, Commands, parse_test_invocation, validate_test_branch_options};

use handlers::{
    dispatch_check, dispatch_cov, dispatch_dry, dispatch_mv, dispatch_rules, dispatch_stats,
    dispatch_test, dispatch_viz,
};
use options::{
    CheckDispatchOptions, CovDispatchOptions, DryDispatchOptions, MvDispatchOptions, MvOutputFlags,
    RulesDispatchOptions, StatsDispatchOptions, TestDispatchOptions, TriConfig, VizDispatchOptions,
};

use kiss::GateConfig;
use kiss::TestSectionConfig;

fn dispatch_analyze(
    lang: Option<kiss::Language>,
    command: Commands,
    cfg: &TriConfig<'_>,
    test_section: &TestSectionConfig,
) -> i32 {
    match command {
        Commands::Check {
            paths,
            ignore,
            timing,
        } => dispatch_check(CheckDispatchOptions {
            lang,
            paths,
            ignore,
            timing,
            cfg,
        }),
        Commands::Coverage {
            paths,
            all,
            ignore,
            timing,
            jobs,
        } => dispatch_cov(CovDispatchOptions {
            lang,
            paths,
            bypass_gate: all,
            ignore,
            timing,
            jobs,
            cfg,
            test_cfg: test_section,
        }),
        Commands::Stats {
            paths,
            all,
            table,
            ignore,
        } => dispatch_stats(StatsDispatchOptions {
            lang,
            paths,
            all,
            table,
            ignore,
            cfg,
        }),
        _ => 2,
    }
}

fn dispatch_tools(
    lang: Option<kiss::Language>,
    defaults: bool,
    config: Option<std::path::PathBuf>,
    command: Commands,
    cfg: &TriConfig<'_>,
    test_section: &TestSectionConfig,
) -> i32 {
    match command {
        Commands::Dry {
            path,
            filter_files,
            shingle_size,
            minhash_size,
            lsh_bands,
            min_similarity,
            ignore,
        } => dispatch_dry(DryDispatchOptions {
            lang,
            path,
            filter_files,
            shingle_size,
            minhash_size,
            lsh_bands,
            min_similarity: min_similarity.unwrap_or(cfg.gate.min_similarity),
            ignore,
            language_tables: cfg.language_tables,
        }),
        Commands::Rules => dispatch_rules(RulesDispatchOptions {
            lang,
            defaults,
            cfg,
        }),
        Commands::Viz {
            out,
            paths,
            zoom,
            num_nodes,
            ignore,
        } => dispatch_viz(VizDispatchOptions {
            lang,
            out,
            paths,
            zoom,
            num_nodes,
            ignore,
            language_tables: cfg.language_tables,
        }),
        test_command @ Commands::Test { .. } => dispatch_test_command(
            lang,
            defaults,
            config.as_ref(),
            test_command,
            cfg,
            test_section,
        ),
        Commands::Mv {
            query,
            new_name,
            paths,
            to,
            dry_run,
            json,
            ignore,
        } => dispatch_mv(MvDispatchOptions {
            lang,
            query,
            new_name,
            paths,
            to,
            mv_flags: MvOutputFlags { dry_run, json },
            ignore,
            language_tables: cfg.language_tables,
        }),
        _ => 2,
    }
}

fn dispatch_test_command(
    lang: Option<kiss::Language>,
    defaults: bool,
    config_path: Option<&std::path::PathBuf>,
    command: Commands,
    cfg: &TriConfig<'_>,
    test_section: &TestSectionConfig,
) -> i32 {
    match command {
        Commands::Test {
            operands,
            main_branch,
            base_branch,
            dry_run,
            force,
            force_bad,
            metrics,
            coverage_all,
            watch,
            jobs,
            ignore,
            extra,
        } => {
            let invocation = match parse_test_invocation(&operands) {
                Ok(invocation) => invocation,
                Err(e) => {
                    eprintln!("error: kiss test: {e}");
                    return 2;
                }
            };
            if let Err(e) = validate_test_branch_options(
                &invocation,
                main_branch.as_deref(),
                base_branch.as_deref(),
            ) {
                eprintln!("error: kiss test: {e}");
                return 2;
            }
            if watch && dry_run {
                eprintln!("error: kiss test: --watch cannot be combined with --dry-run");
                return 2;
            }
            dispatch_test(TestDispatchOptions {
                lang,
                invocation,
                main_branch,
                base_branch,
                dry_run,
                force,
                force_bad,
                metrics,
                coverage_all,
                watch,
                jobs,
                ignore,
                extra,
                test_cfg: test_section,
                cfg,
                reload_kissconfig: !defaults,
                config_path,
            })
        }
        _ => 2,
    }
}

#[allow(clippy::too_many_lines)]
pub fn dispatch(
    cli: Cli,
    py_config: &kiss::Config,
    rs_config: &kiss::Config,
    gate_config: &GateConfig,
    test_section: &TestSectionConfig,
) -> i32 {
    let language_tables =
        crate::bin_cli::config_session::load_language_tables(cli.config.as_ref(), cli.defaults);
    let cfg = TriConfig {
        py: py_config,
        rs: rs_config,
        gate: gate_config,
        language_tables,
    };
    match cli {
        Cli {
            lang,
            defaults: _,
            config: _,
            command:
                command @ (Commands::Check { .. } | Commands::Coverage { .. } | Commands::Stats { .. }),
        } => dispatch_analyze(lang, command, &cfg, test_section),
        Cli {
            lang,
            defaults,
            config,
            command,
        } => dispatch_tools(lang, defaults, config, command, &cfg, test_section),
    }
}
