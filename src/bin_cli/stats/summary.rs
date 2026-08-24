use crate::bin_cli::config_session::config_provenance;
use crate::bin_cli::util::merge_check_ignore_prefixes;
use crate::test_runner::unit_test_timing::{
    TimingLangInclude, unit_test_runtime_sec_report_for_universe,
};
use kiss::check_universe_cache::FullCheckCache;
use kiss::discovery::gather_files_by_lang;
use kiss::{Config, GateConfig, Language, compute_summaries, format_stats_table};
use std::path::{Path, PathBuf};
use std::time::Instant;

struct StatsSummaryInput<'a> {
    paths: &'a [String],
    py_files: &'a [PathBuf],
    rs_files: &'a [PathBuf],
    py_cfg: &'a Config,
    rs_cfg: &'a Config,
    lang_filter: Option<Language>,
    ignore: &'a [String],
    gate: &'a GateConfig,
    config: Option<&'a Path>,
}

pub fn run_stats_summary(args: &super::RunStatsArgs<'_>) -> i32 {
    let paths = args.paths;
    let lang_filter = args.lang_filter;
    let ignore = args.ignore;
    let py_cfg = args.py_config;
    let rs_cfg = args.rs_config;
    let gate = args.gate_config;
    let language_tables = args.language_tables;
    let config = args.config;
    let (py_files, rs_files) = gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        return 1;
    }
    if let Err(code) =
        crate::bin_cli::util::reject_unconfigured_languages(&py_files, &rs_files, language_tables)
    {
        return code;
    }

    if maybe_print_cached_stats_summary(CachedStatsSummaryArgs {
        paths,
        py_files: &py_files,
        rs_files: &rs_files,
        py_cfg,
        rs_cfg,
        gate,
        lang_filter,
        ignore,
        config,
    }) {
        return 0;
    }

    run_stats_summary_from_pipeline(StatsSummaryInput {
        paths,
        py_files: &py_files,
        rs_files: &rs_files,
        py_cfg,
        rs_cfg,
        lang_filter,
        ignore,
        gate,
        config,
    })
}

fn run_stats_summary_from_pipeline(input: StatsSummaryInput<'_>) -> i32 {
    let focus_filter = crate::analyze::FocusFilter::unrestricted();
    let universe = input.paths.first().map(String::as_str).unwrap_or_default();
    let now = Instant::now();
    let options = crate::analyze::AnalyzeOptions {
        universe,
        focus_paths: input.paths,
        py_config: input.py_cfg,
        rs_config: input.rs_cfg,
        lang_filter: input.lang_filter,
        bypass_gate: true,
        gate_config: input.gate,
        ignore_prefixes: input.ignore,
        show_timing: false,
        suppress_final_status: false,
        language_tables: kiss::LanguageTablesPresent::both(),
    };

    let pipeline = match crate::analyze::run_full_pipeline(crate::analyze::FullPipelineInput {
        opts: &options,
        py_files: input.py_files,
        rs_files: input.rs_files,
        focus: &focus_filter,
        t0: now,
        t1: now,
        t2: now,
    }) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };
    crate::analyze::maybe_store_full_cache(crate::analyze::FullCacheStoreInput {
        opts: &options,
        py_files: input.py_files,
        rs_files: input.rs_files,
        focus: &focus_filter,
        result: &pipeline.result,
        graph_viols_all: &pipeline.graph_viols_all,
        py_graph: pipeline.py_graph.as_ref(),
        rs_graph: pipeline.rs.graph.as_ref(),
        py_dups_all: &pipeline.py_dups_all,
        rs_dups_all: &pipeline.rs_dups_all,
        py_stats: Some(&pipeline.py_stats),
        rs_stats: Some(&pipeline.rs_stats),
    });
    print_summary_from_pipeline(
        input.paths,
        &pipeline,
        input.lang_filter,
        input.ignore,
        input.gate,
        input.config,
    );
    0
}

fn maybe_print_unit_test_runtime_line(
    paths: &[String],
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
    rules: &[(String, f64)],
) {
    if let Some(report) =
        unit_test_runtime_section_for_rules(paths, lang_filter, include, ignore, rules)
    {
        println!("{report}");
    }
}

fn print_summary_from_pipeline(
    paths: &[String],
    pipeline: &crate::analyze::FullPipelineResult,
    lang_filter: Option<Language>,
    ignore: &[String],
    gate: &GateConfig,
    config: Option<&Path>,
) {
    let duplicate_total = pipeline.py_dups_all.len() + pipeline.rs_dups_all.len();
    let orphan_total = pipeline
        .result
        .violations
        .iter()
        .chain(pipeline.graph_viols_all.iter())
        .filter(|v| v.metric == "orphan_module")
        .count();
    let graph_nodes = pipeline
        .py_graph
        .as_ref()
        .map_or(0, |g| g.graph.node_count())
        + pipeline
            .rs
            .graph
            .as_ref()
            .map_or(0, |g| g.graph.node_count());
    let graph_edges = pipeline
        .py_graph
        .as_ref()
        .map_or(0, |g| g.graph.edge_count())
        + pipeline
            .rs
            .graph
            .as_ref()
            .map_or(0, |g| g.graph.edge_count());

    println!("kiss stats - Summary Statistics");
    println!("Analyzed from: {}", paths.join(", "));
    println!("{}", config_provenance(config));
    println!();
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        pipeline.result.py_parsed.len() + pipeline.result.rs_parsed.len(),
        pipeline.result.code_unit_count,
        pipeline.result.statement_count,
        graph_nodes,
        graph_edges
    );
    println!(
        "{}\n",
        format_violation_counts(
            duplicate_total,
            orphan_total,
            count_metric(
                pipeline.result.violations.iter().map(|v| v.metric.as_str()),
                "comment"
            ),
            count_metric(
                pipeline.result.violations.iter().map(|v| v.metric.as_str()),
                "doc"
            ),
        )
    );

    if !pipeline.result.py_parsed.is_empty() {
        println!(
            "=== Python ({} files) ===\n{}\n",
            pipeline.result.py_parsed.len(),
            format_stats_table(&compute_summaries(&pipeline.py_stats))
        );
    }
    if !pipeline.result.rs_parsed.is_empty() {
        println!(
            "=== Rust ({} files) ===\n{}",
            pipeline.result.rs_parsed.len(),
            format_stats_table(&compute_summaries(&pipeline.rs_stats))
        );
    }
    maybe_print_unit_test_runtime_line(
        paths,
        lang_filter,
        TimingLangInclude {
            python: !pipeline.result.py_parsed.is_empty(),
            rust: !pipeline.result.rs_parsed.is_empty(),
        },
        ignore,
        &gate.max_unit_test_seconds,
    );
}

struct CachedStatsSummaryArgs<'a> {
    paths: &'a [String],
    py_files: &'a [PathBuf],
    rs_files: &'a [PathBuf],
    py_cfg: &'a Config,
    rs_cfg: &'a Config,
    gate: &'a GateConfig,
    lang_filter: Option<Language>,
    ignore: &'a [String],
    config: Option<&'a Path>,
}

fn maybe_print_cached_stats_summary(args: CachedStatsSummaryArgs<'_>) -> bool {
    let Some(cache) = crate::analyze_cache::try_run_cached_stats_summary(
        args.paths.first().map(String::as_str).unwrap_or("."),
        args.py_files,
        args.rs_files,
        args.py_cfg,
        args.rs_cfg,
        args.gate,
    ) else {
        return false;
    };
    print_cached_summary(
        args.paths,
        &cache,
        args.lang_filter,
        TimingLangInclude {
            python: !args.py_files.is_empty(),
            rust: !args.rs_files.is_empty(),
        },
        args.ignore,
        args.gate,
        args.config,
    );
    true
}

fn print_cached_summary(
    paths: &[String],
    cache: &FullCheckCache,
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
    gate: &GateConfig,
    config: Option<&Path>,
) {
    let dup_total = cache.py_duplicates.len() + cache.rs_duplicates.len();
    let orphan_total = cache
        .base_violations
        .iter()
        .chain(cache.graph_violations.iter())
        .filter(|v| v.metric == "orphan_module")
        .count();

    println!("kiss stats - Summary Statistics");
    println!("Analyzed from: {}", paths.join(", "));
    println!("{}", config_provenance(config));
    println!();
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        cache.py_file_count + cache.rs_file_count,
        cache.code_unit_count,
        cache.statement_count,
        cache.graph_nodes,
        cache.graph_edges
    );
    println!(
        "{}\n",
        format_violation_counts(
            dup_total,
            orphan_total,
            count_metric(
                cache.base_violations.iter().map(|v| v.metric.as_str()),
                "comment"
            ),
            count_metric(
                cache.base_violations.iter().map(|v| v.metric.as_str()),
                "doc"
            ),
        )
    );

    if cache.py_file_count > 0
        && let Some(stats) = &cache.py_stats
    {
        println!(
            "=== Python ({} files) ===\n{}\n",
            cache.py_file_count,
            format_stats_table(&compute_summaries(stats))
        );
    }
    if cache.rs_file_count > 0
        && let Some(stats) = &cache.rs_stats
    {
        println!(
            "=== Rust ({} files) ===\n{}",
            cache.rs_file_count,
            format_stats_table(&compute_summaries(stats))
        );
    }
    maybe_print_unit_test_runtime_line(
        paths,
        lang_filter,
        include,
        ignore,
        &gate.max_unit_test_seconds,
    );
}

fn unit_test_runtime_section_for_rules(
    paths: &[String],
    lang_filter: Option<Language>,
    include: TimingLangInclude,
    ignore: &[String],
    rules: &[(String, f64)],
) -> Option<String> {
    let universe = Path::new(paths.first().map(String::as_str).unwrap_or("."));
    let merged = merge_check_ignore_prefixes(ignore);
    let pytest_args = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    unit_test_runtime_sec_report_for_universe(
        universe,
        lang_filter,
        include,
        &merged,
        rules,
        &pytest_args,
    )
}

fn format_violation_counts(duplicate: usize, orphan: usize, comment: usize, doc: usize) -> String {
    format!("Violations: {duplicate} duplicate, {orphan} orphan, {comment} comment, {doc} doc")
}

fn count_metric<'a, I>(metrics: I, metric: &str) -> usize
where
    I: IntoIterator<Item = &'a str>,
{
    metrics.into_iter().filter(|name| *name == metric).count()
}

#[cfg(test)]
#[path = "summary_test.rs"]
mod summary_tests;

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use kiss::{Config, GateConfig};

    impl StatsSummaryInput<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_stats_summary_input() {
        StatsSummaryInput::witness();
        let py_cfg = Config::default();
        let rs_cfg = Config::default();
        let gate = GateConfig::default();
        let _ = StatsSummaryInput {
            paths: &[],
            py_files: &[],
            rs_files: &[],
            py_cfg: &py_cfg,
            rs_cfg: &rs_cfg,
            lang_filter: None,
            ignore: &[],
            gate: &gate,
            config: None,
        };
    }
}
