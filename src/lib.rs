//! kiss - Code-quality metrics tool for Python and Rust

// Shared modules
pub mod cli_output;
pub mod config;
pub mod config_gen;
pub mod defaults;
pub mod py_metrics;
pub mod violation;

// Python modules
pub mod counts;
pub mod discovery;
pub mod duplication;
pub mod graph;
pub mod parsing;
pub mod stats;
pub mod test_refs;
pub mod units;

// Rust modules
pub mod rust_counts;
pub mod rust_graph;
pub mod rust_parsing;
pub mod rust_test_refs;
pub mod rust_units;

// Re-export main types and functions for easy access
pub use config::{Config, ConfigLanguage, GateConfig};
pub use defaults::default_config_toml;
pub use counts::analyze_file;
pub use py_metrics::{
    compute_class_metrics, compute_class_metrics_with_source, compute_file_metrics,
    compute_function_metrics, ClassMetrics, FileMetrics, FunctionMetrics,
};
pub use violation::{Violation, ViolationBuilder};
pub use discovery::{find_python_files, find_rust_files, find_source_files, Language, SourceFile};
pub use duplication::{
    cluster_duplicates, detect_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication, CodeChunk,
    DuplicateCluster, DuplicatePair, DuplicationConfig, MinHashSignature,
};
pub use graph::{
    analyze_graph, build_dependency_graph, collect_instability_metrics,
    compute_cyclomatic_complexity, CycleInfo, DependencyGraph, InstabilityMetric,
    ModuleGraphMetrics,
};
pub use parsing::{create_parser, parse_file, parse_files, ParseError, ParsedFile};
pub use stats::{
    compute_summaries, format_stats_table, generate_config_toml, MetricStats, PercentileSummary,
};
pub use test_refs::{analyze_test_refs, is_test_file, CodeDefinition, TestRefAnalysis};
pub use units::{extract_code_units, CodeUnit, CodeUnitKind};

// Rust re-exports
pub use rust_counts::{analyze_rust_file, RustFileMetrics, RustFunctionMetrics, RustTypeMetrics};
pub use rust_graph::build_rust_dependency_graph;
pub use rust_parsing::{parse_rust_file, parse_rust_files, ParsedRustFile, RustParseError};
pub use rust_test_refs::{
    analyze_rust_test_refs, is_rust_test_file, RustCodeDefinition, RustTestRefAnalysis,
};
pub use rust_units::{extract_rust_code_units, RustCodeUnit};
