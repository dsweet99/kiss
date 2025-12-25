//! kiss - Code-quality metrics tool for Python and Rust

// Allow some pedantic clippy lints that are acceptable in this codebase
#![allow(clippy::cast_precision_loss)] // usize to f64 for percentages
#![allow(clippy::cast_possible_truncation)] // f64 to usize for percentages
#![allow(clippy::cast_sign_loss)] // f64 to usize for percentages
#![allow(clippy::struct_field_names)] // field names matching struct name
#![allow(clippy::module_name_repetitions)] // types named after modules
#![allow(clippy::similar_names)] // similar variable names
#![allow(clippy::too_many_lines)] // long functions are sometimes necessary
#![allow(clippy::field_reassign_with_default)] // common pattern in tests
#![allow(clippy::format_push_string)] // acceptable for simple string building
#![allow(clippy::return_self_not_must_use)] // builders don't need must_use
#![allow(clippy::needless_update)] // explicit ..Default::default() for clarity
#![allow(clippy::iter_on_single_items)] // acceptable for consistency
#![allow(clippy::float_cmp)] // acceptable for test assertions
#![allow(clippy::implicit_hasher)] // HashSet<String> is fine without generalization
#![allow(clippy::case_sensitive_file_extension_comparisons)] // .py files are always lowercase

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
pub mod minhash;
pub mod parsing;
pub mod stats;
pub mod test_refs;
pub mod units;

// Rust modules
pub mod rust_counts;
pub mod rust_fn_metrics;
pub mod rust_graph;
pub mod rust_lcom;
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
pub use discovery::{find_python_files, find_rust_files, find_source_files, find_source_files_with_ignore, Language, SourceFile};
pub use duplication::{
    cluster_duplicates, detect_duplicates, detect_duplicates_from_chunks,
    extract_chunks_for_duplication, extract_rust_chunks_for_duplication, CodeChunk,
    DuplicateCluster, DuplicatePair, DuplicationConfig, MinHashSignature,
};
pub use graph::{
    analyze_graph, build_dependency_graph,
    compute_cyclomatic_complexity, CycleInfo, DependencyGraph,
    ModuleGraphMetrics,
};
pub use parsing::{create_parser, parse_file, parse_files, ParseError, ParsedFile};
pub use stats::{
    compute_summaries, format_stats_table, generate_config_toml, MetricStats, PercentileSummary,
};
pub use test_refs::{analyze_test_refs, is_test_file, CodeDefinition, TestRefAnalysis};
pub use units::{extract_code_units, CodeUnit, CodeUnitKind};

// Rust re-exports
pub use rust_counts::analyze_rust_file;
pub use rust_fn_metrics::{
    compute_rust_file_metrics, compute_rust_function_metrics, RustFileMetrics,
    RustFunctionMetrics, RustTypeMetrics,
};
pub use rust_graph::build_rust_dependency_graph;
pub use rust_lcom::compute_rust_lcom;
pub use rust_parsing::{parse_rust_file, parse_rust_files, ParsedRustFile, RustParseError};
pub use rust_test_refs::{
    analyze_rust_test_refs, is_rust_test_file, RustCodeDefinition, RustTestRefAnalysis,
};
pub use rust_units::{extract_rust_code_units, RustCodeUnit};
