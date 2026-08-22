#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod cli_output;
pub mod config;
pub mod config_gen;
pub mod defaults;
pub mod gate_config;
pub mod py_imports;
pub mod py_metrics;
pub mod shared_helpers;
pub mod violation;

pub mod check_cache;
pub mod check_universe_cache;
pub mod comments;
pub mod counts;
pub mod discovery;
pub mod duplication;
pub mod graph;
pub mod layout_cycles;
mod macro_expr_parser;
pub mod minhash;
pub mod parsing;
pub mod stats;
pub mod stats_detailed;
pub mod symbol_mv;
pub mod test_refs;
pub mod test_section_config;
pub mod units;

pub mod code_roles;
pub mod lang_analysis;
pub mod rust_counts;
pub mod rust_fn_metrics;
pub mod rust_graph;
pub mod rust_include;
pub mod rust_parsing;
pub mod rust_test_refs;
pub mod rust_units;

pub mod global_metrics;
pub mod layout_layers;
pub mod layout_output;

pub(crate) mod symbol_mv_support;

#[cfg(test)]
pub mod test_utils;

pub use cli_output::print_dry_results;
pub use comments::{
    COMMENT_METRIC, DOC_METRIC, collect_comment_violations, collect_comment_violations_with_roles,
    collect_doc_violations, collect_doc_violations_with_roles, has_non_doc_comments,
    has_non_doc_comments_with_roles,
};
pub use config::{
    Config, ConfigError, ConfigLanguage, LanguageTablesPresent, find_repo_root, is_similar,
    kissconfig_path_for_repo, kissconfig_path_from_cwd, missing_language_table_message,
    reject_unconfigured_languages,
};
pub use counts::analyze_file;
pub use counts::analyze_file_with_statement_count;
pub use defaults::default_config_toml;
pub use discovery::{
    DEFAULT_CHECK_IGNORE_PREFIXES, Language, SourceFile, default_check_ignore_prefixes,
    find_python_files, find_rust_files, find_source_files, find_source_files_with_ignore,
    gather_files_by_lang, gather_files_by_lang_opts, merge_check_ignore_prefixes,
    normalize_ignore_prefixes,
};
pub use duplication::{
    CodeChunk, DuplicateCluster, DuplicatePair, DuplicationConfig, MinHashSignature,
    cluster_duplicates, cluster_duplicates_from_chunks, detect_duplicates,
    detect_duplicates_from_chunks, extract_chunks_for_duplication,
    extract_chunks_for_duplication_with_roles, extract_rust_chunks_for_duplication,
    extract_rust_chunks_for_duplication_with_roles,
};
pub use gate_config::{
    GateConfig, MatchedUnitTestSecondsRule, TestCoverageScope, catch_all_limit, exceeds_limit,
    format_nested_toml_table, limit_for_selector, matched_rule_for_selector,
};
pub use graph::{
    ContextDependencyGraph, CycleInfo, DependencyGraph, EdgeOrigin, GraphKeyMaxima,
    ModuleGraphMetrics, RoleDependencyGraphs, analyze_graph, build_dependency_graph,
    build_python_context_graph, compute_cyclomatic_complexity, graph_key_maxima,
    module_name_for_path, path_for_module_name,
};
pub use layout_cycles::{CycleBreakSuggestion, LayoutCycleAnalysis, analyze_cycles};
pub use layout_layers::{LayerInfo, compute_layers};
pub use layout_output::{LayoutAnalysis, LayoutMetrics, WhatIfAnalysis, format_markdown};
pub use parsing::{ParseError, ParsedFile, create_parser, parse_file, parse_files};
pub use py_metrics::{
    ClassMetrics, FileMetrics, FunctionMetrics, compute_class_metrics, compute_file_metrics,
    compute_function_metrics,
};
pub use shared_helpers::{
    env_map_from_allowlist, json_entry_paths, python_coverage_env_map,
    pythonpath_for_coverage_identity, scrubbed_git_command,
};
pub use stats::{
    METRICS, MetricDef, MetricScope, MetricStats, PercentileSummary, compute_summaries,
    format_stats_table, get_metric_def,
};
pub use stats_detailed::{
    UnitMetrics, collect_detailed_py, collect_detailed_rs, collect_detailed_rs_with_roles,
    format_detailed_table, truncate,
};
pub use test_refs::is_in_test_directory;
pub use test_section_config::{
    TestSectionConfig, effective_python_pytest_args, pytest_plugin_cli_args,
};
pub use units::count_code_units;
pub use units::{CodeUnit, CodeUnitKind, extract_code_units};
pub use violation::{Violation, ViolationBuilder};

pub use rust_counts::{
    analyze_rust_file, analyze_rust_file_include_rollup,
    analyze_rust_file_include_rollup_with_roles, analyze_rust_file_with_roles,
};
pub use rust_fn_metrics::{
    RustFileMetrics, RustFunctionMetrics, RustTypeMetrics, compute_rust_file_metrics,
    compute_rust_file_metrics_with_roles, compute_rust_function_metrics, count_non_doc_attrs,
};
pub use rust_graph::{
    IncludeGraph, build_include_graph, build_rust_context_graph, build_rust_dependency_graph,
    build_rust_dependency_graph_with_roles, expand_rust_files,
};
pub use rust_parsing::{ParsedRustFile, RustParseError, parse_rust_file, parse_rust_files};
pub use rust_test_refs::is_binary_entry_point;
pub use rust_units::{RustCodeUnit, extract_rust_code_units};

pub use code_roles::{
    CodeRole, FileComposition, RoleBuildError, SourceRoleIndex,
    is_default_pytest_collect_candidate, is_python_test_module_path, is_test_only_file,
};
pub use global_metrics::GlobalMetrics;

#[cfg(test)]
pub mod cwd_test_lock {
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    pub fn lock() -> std::sync::MutexGuard<'static, ()> {
        LOCK.lock().unwrap()
    }
}
