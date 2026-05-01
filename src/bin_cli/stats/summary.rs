use crate::bin_cli::config_session::{config_provenance, load_configs, load_gate_config};
use kiss::check_universe_cache::FullCheckCache;
use kiss::{
    Config, GateConfig, Language, compute_summaries, find_source_files_with_ignore, format_stats_table,
};
use std::collections::HashSet;
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
}

pub fn run_stats_summary(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    let (py_files, rs_files) = collect_files(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }

    let gate = load_gate_config(None, false);
    let (py_cfg, rs_cfg) = load_configs(None, false);

    if try_run_cached_stats_summary(paths, &py_files, &rs_files, &py_cfg, &rs_cfg, &gate) {
        return;
    }

    run_stats_summary_from_pipeline(StatsSummaryInput {
        paths,
        py_files: &py_files,
        rs_files: &rs_files,
        py_cfg: &py_cfg,
        rs_cfg: &rs_cfg,
        lang_filter,
        ignore,
        gate: &gate,
    });
}

fn run_stats_summary_from_pipeline(input: StatsSummaryInput<'_>) {
    let mut focus_set = HashSet::new();
    focus_set.extend(input.py_files.iter().cloned());
    focus_set.extend(input.rs_files.iter().cloned());
    let universe = input.paths.first().map(String::as_str).unwrap_or_default();
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
    };

    let pipeline = crate::analyze::run_full_pipeline(crate::analyze::FullPipelineInput {
        opts: &options,
        py_files: input.py_files,
        rs_files: input.rs_files,
        focus_set: &focus_set,
        t0: Instant::now(),
        t1: Instant::now(),
        t2: Instant::now(),
    });
    crate::analyze::maybe_store_full_cache(crate::analyze::FullCacheStoreInput {
        opts: &options,
        py_files: input.py_files,
        rs_files: input.rs_files,
        focus_set: &focus_set,
        result: &pipeline.result,
        graph_viols_all: &pipeline.graph_viols_all,
        coverage_violations: &pipeline.cov_viols,
        py_graph: pipeline.py_graph.as_ref(),
        rs_graph: pipeline.rs.graph.as_ref(),
        py_dups_all: &pipeline.py_dups_all,
        rs_dups_all: &pipeline.rs_dups_all,
        coverage_cache_lists: pipeline.coverage_cache_lists.clone(),
        py_stats: Some(&pipeline.py_stats),
        rs_stats: Some(&pipeline.rs_stats),
    });
    print_summary_from_pipeline(input.paths, &pipeline);
}

fn print_summary_from_pipeline(paths: &[String], pipeline: &crate::analyze::FullPipelineResult) {
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
    println!("{}", config_provenance());
    println!();
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        pipeline.result.py_parsed.len() + pipeline.result.rs_parsed.len(),
        pipeline.result.code_unit_count,
        pipeline.result.statement_count,
        graph_nodes,
        graph_edges
    );
    println!("Violations: {duplicate_total} duplicate, {orphan_total} orphan\n");

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
}

fn try_run_cached_stats_summary(
    paths: &[String],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_cfg: &Config,
    rs_cfg: &Config,
    gate: &GateConfig,
) -> bool {
    let Some(cache) = crate::analyze_cache::try_run_cached_stats_summary(
        py_files,
        rs_files,
        py_cfg,
        rs_cfg,
        gate,
    ) else {
        return false;
    };
    print_cached_summary(paths, &cache);
    true
}

fn collect_files(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut py_files: Vec<PathBuf> = Vec::new();
    let mut rs_files: Vec<PathBuf> = Vec::new();
    for path in paths {
        let root = Path::new(path);
        for sf in find_source_files_with_ignore(root, ignore) {
            let want = lang_filter.is_none() || lang_filter == Some(sf.language);
            if !want {
                continue;
            }
            match sf.language {
                Language::Python => py_files.push(sf.path),
                Language::Rust => rs_files.push(sf.path),
            }
        }
    }
    (py_files, rs_files)
}

fn print_cached_summary(paths: &[String], cache: &FullCheckCache) {
    let dup_total = cache.py_duplicates.len() + cache.rs_duplicates.len();
    let orphan_total = cache
        .base_violations
        .iter()
        .chain(cache.graph_violations.iter())
        .filter(|v| v.metric == "orphan_module")
        .count();
    let py_file_count = cache.py_file_count;
    let rs_file_count = cache.rs_file_count;

    println!("kiss stats - Summary Statistics");
    println!("Analyzed from: {}", paths.join(", "));
    println!("{}", config_provenance());
    println!();
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        cache.py_file_count + cache.rs_file_count,
        cache.code_unit_count,
        cache.statement_count,
        cache.graph_nodes,
        cache.graph_edges
    );
    println!("Violations: {dup_total} duplicate, {orphan_total} orphan\n");

    if py_file_count > 0
        && let Some(stats) = &cache.py_stats
    {
            println!(
                "=== Python ({} files) ===\n{}\n",
                py_file_count,
                format_stats_table(&compute_summaries(stats))
            );
    }
    if rs_file_count > 0
        && let Some(stats) = &cache.rs_stats
    {
            println!(
                "=== Rust ({} files) ===\n{}",
                rs_file_count,
                format_stats_table(&compute_summaries(stats))
            );
    }
}
