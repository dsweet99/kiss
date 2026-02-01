pub mod cli_output;
pub mod config;
pub mod config_gen;
pub mod defaults;
pub mod gate_config;
pub mod py_imports;
pub mod py_metrics;
pub mod rule_defs;
pub mod violation;

pub mod counts;
pub mod discovery;
pub mod duplication;
pub mod graph;
pub mod minhash;
pub mod parsing;
pub mod stats;
pub mod stats_detailed;
pub mod test_refs;
pub mod units;

pub mod rust_counts;
pub mod rust_fn_metrics;
pub mod rust_graph;
pub mod rust_parsing;
pub mod rust_test_refs;
pub mod rust_units;

#[cfg(test)]
pub mod test_utils;

pub use cli_output::print_dry_results;
pub use config::{Config, ConfigLanguage, is_similar};
pub use counts::analyze_file;
pub use defaults::default_config_toml;
pub use discovery::{
    Language, SourceFile, find_python_files, find_rust_files, find_source_files,
    find_source_files_with_ignore, gather_files_by_lang,
};
pub use duplication::{
    CodeChunk, DuplicateCluster, DuplicatePair, DuplicationConfig, MinHashSignature,
    cluster_duplicates, detect_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication,
};
pub use gate_config::GateConfig;
pub use graph::{
    CycleInfo, DependencyGraph, ModuleGraphMetrics, analyze_graph, build_dependency_graph,
    compute_cyclomatic_complexity,
};
pub use parsing::{ParseError, ParsedFile, create_parser, parse_file, parse_files};
pub use py_metrics::{
    ClassMetrics, FileMetrics, FunctionMetrics, compute_class_metrics, compute_file_metrics,
    compute_function_metrics,
};
pub use stats::{
    METRICS, MetricDef, MetricScope, MetricStats, PercentileSummary, compute_summaries,
    format_stats_table, generate_config_toml, get_metric_def,
};
pub use stats_detailed::{
    UnitMetrics, collect_detailed_py, collect_detailed_rs, format_detailed_table, truncate,
};
pub use test_refs::{CodeDefinition, TestRefAnalysis, analyze_test_refs, is_test_file};
pub use units::{CodeUnit, CodeUnitKind, extract_code_units};
pub use violation::{Violation, ViolationBuilder};

pub use rust_counts::analyze_rust_file;
pub use rust_fn_metrics::{
    RustFileMetrics, RustFunctionMetrics, RustTypeMetrics, compute_rust_file_metrics,
    compute_rust_function_metrics,
};
pub use rust_graph::build_rust_dependency_graph;
pub use rust_parsing::{ParsedRustFile, RustParseError, parse_rust_file, parse_rust_files};
pub use rust_test_refs::{
    RustCodeDefinition, RustTestRefAnalysis, analyze_rust_test_refs, is_rust_test_file,
};
pub use rust_units::{RustCodeUnit, extract_rust_code_units};

pub use rule_defs::{Applicability, RULES, Rule, RuleCategory, rules_for_python, rules_for_rust};
