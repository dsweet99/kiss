mod handlers;
mod options;

#[cfg(test)]
mod test_dispatch;
#[cfg(test)]
mod test_dispatch_b;

use crate::bin_cli::args::{Cli, Commands, parse_test_invocation, validate_test_branch_options};

use handlers::{
    dispatch_check, dispatch_dry, dispatch_rules, dispatch_stats, dispatch_test, dispatch_viz,
};
use options::{
    CheckDispatchOptions, DryDispatchOptions, RulesDispatchOptions, StatsDispatchOptions,
    TestDispatchOptions, TriConfig, VizDispatchOptions,
};

use kiss::GateConfig;
use kiss::TestSectionConfig;

fn dispatch_analyze(
    lang: Option<kiss::Language>,
    config: Option<std::path::PathBuf>,
    command: Commands,
    cfg: &TriConfig<'_>,
    test_section: &TestSectionConfig,
) -> i32 {
    match command {
        Commands::Check {
            paths,
            ignore,
            timing,
        } => {
            let ignore = test_section.merged_ignore(&ignore);
            dispatch_check(CheckDispatchOptions {
                lang,
                paths,
                ignore,
                timing,
                config,
                cfg,
            })
        }
        Commands::Stats {
            paths,
            all,
            table,
            ignore,
        } => {
            let ignore = test_section.merged_ignore(&ignore);
            dispatch_stats(StatsDispatchOptions {
                lang,
                paths,
                all,
                table,
                ignore,
                cfg,
                config,
            })
        }
        _ => 2,
    }
}

fn dispatch_tools(
    lang: Option<kiss::Language>,
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
        Commands::Rules => dispatch_rules(RulesDispatchOptions { lang, cfg }),
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
        test_command @ Commands::Test { .. } => {
            dispatch_test_command(lang, test_command, cfg, test_section)
        }
        _ => 2,
    }
}

fn absolutize_test_invocation(
    invocation: crate::bin_cli::args::TestInvocation,
) -> crate::bin_cli::args::TestInvocation {
    use crate::bin_cli::args::TestInvocation;
    match invocation {
        TestInvocation::Targets(operands) => {
            TestInvocation::Targets(operands.into_iter().map(absolutize_operand).collect())
        }
        other => other,
    }
}

fn absolutize_operand(raw: String) -> String {
    let (path, symbol) = match raw.split_once("::") {
        Some((path, symbol)) => (path.to_string(), Some(symbol.to_string())),
        None => (raw.clone(), None),
    };
    let path_ref = std::path::Path::new(&path);
    if path_ref.is_absolute() {
        return raw;
    }
    let Ok(cwd) = std::env::current_dir() else {
        return raw;
    };
    let abs = cwd.join(path_ref);
    match symbol {
        Some(symbol) => format!("{}::{symbol}", abs.display()),
        None => abs.display().to_string(),
    }
}

fn dispatch_test_command(
    lang: Option<kiss::Language>,
    command: Commands,
    cfg: &TriConfig<'_>,
    test_section: &TestSectionConfig,
) -> i32 {
    match command {
        Commands::Test {
            operands,
            main_branch,
            base_branch,
            retry_bad,
            jobs,
        } => {
            if let Some(operand) = operands.iter().find(|operand| operand.starts_with('-')) {
                eprintln!("error: kiss test: arguments after -- are not accepted: {operand}");
                return 2;
            }
            let invocation = match parse_test_invocation(&operands) {
                Ok(invocation) => absolutize_test_invocation(invocation),
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
            dispatch_test(TestDispatchOptions {
                lang,
                invocation,
                main_branch,
                base_branch,
                dry_run: false,
                retry_bad,
                metrics: false,
                coverage_all: false,
                jobs,
                ignore: Vec::new(),
                extra: Vec::new(),
                test_cfg: test_section,
                cfg,
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
    let language_tables = crate::bin_cli::config_session::load_language_tables(cli.config.as_ref());
    let cfg = TriConfig {
        py: py_config,
        rs: rs_config,
        gate: gate_config,
        language_tables,
    };
    match cli {
        Cli {
            lang,
            config,
            command: command @ (Commands::Check { .. } | Commands::Stats { .. }),
        } => dispatch_analyze(lang, config, command, &cfg, test_section),
        Cli { lang, command, .. } => dispatch_tools(lang, command, &cfg, test_section),
    }
}
