//! Command dispatch for the `kiss` binary.

mod handlers;
mod options;

#[cfg(test)]
mod test_dispatch;

use crate::bin_cli::args::{Cli, Commands, parse_test_command_action};
use crate::bin_cli::config_session::run_init_command;

use handlers::{
    dispatch_check, dispatch_clamp, dispatch_config, dispatch_dry, dispatch_mimic, dispatch_mv,
    dispatch_rules, dispatch_shrink, dispatch_stats, dispatch_test, dispatch_viz,
};
use options::{
    CheckDispatchOptions, ConfigDispatchOptions, DryDispatchOptions, MimicDispatchOptions,
    MvDispatchOptions, MvOutputFlags, RulesDispatchOptions, ShrinkDispatchOptions,
    StatsDispatchOptions, TestDispatchOptions, TriConfig, VizDispatchOptions,
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
            all,
            ignore,
            timing,
            jobs,
        } => dispatch_check(CheckDispatchOptions {
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
        Commands::Mimic { paths, out, ignore } => dispatch_mimic(MimicDispatchOptions {
            lang,
            paths,
            out,
            ignore,
        }),
        Commands::Clamp { ignore } => dispatch_clamp(lang, ignore),
        Commands::Init { repo_path } => run_init_command(&repo_path),
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
            min_similarity,
            ignore,
        }),
        Commands::Rules => dispatch_rules(RulesDispatchOptions {
            lang,
            defaults,
            cfg,
        }),
        Commands::Config => dispatch_config(ConfigDispatchOptions {
            defaults,
            config,
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
        }),
        Commands::Shrink {
            target,
            paths,
            ignore,
        } => dispatch_shrink(ShrinkDispatchOptions {
            lang,
            target,
            paths,
            ignore,
            cfg,
        }),
        test_command @ Commands::Test { .. } => {
            dispatch_test_command(lang, test_command, test_section)
        }
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
        }),
        _ => 2,
    }
}

fn dispatch_test_command(
    lang: Option<kiss::Language>,
    command: Commands,
    test_section: &TestSectionConfig,
) -> i32 {
    let Commands::Test {
        mode,
        validation_mode,
        main_branch,
        base_branch,
        dry_run,
        force,
        metrics,
        jobs,
        ignore,
        fixture,
        extra,
    } = command
    else {
        return 2;
    };
    let action = match parse_test_command_action(&mode, validation_mode) {
        Ok(action) => action,
        Err(e) => {
            eprintln!("error: kiss test: {e}");
            return 2;
        }
    };
    dispatch_test(TestDispatchOptions {
        lang,
        action,
        main_branch,
        base_branch,
        dry_run,
        force,
        metrics,
        jobs,
        ignore,
        fixture,
        extra,
        test_cfg: test_section,
    })
}

#[allow(clippy::too_many_lines)]
pub fn dispatch(
    cli: Cli,
    py_config: &kiss::Config,
    rs_config: &kiss::Config,
    gate_config: &GateConfig,
    test_section: &TestSectionConfig,
) -> i32 {
    let cfg = TriConfig {
        py: py_config,
        rs: rs_config,
        gate: gate_config,
    };
    let Cli {
        lang,
        defaults,
        config,
        command,
    } = cli;
    match command {
        Commands::Check { .. }
        | Commands::Stats { .. }
        | Commands::Mimic { .. }
        | Commands::Clamp { .. }
        | Commands::Init { .. } => dispatch_analyze(lang, command, &cfg, test_section),
        _ => dispatch_tools(lang, defaults, config, command, &cfg, test_section),
    }
}
