pub mod cli_output;
pub mod config;
pub mod config_gen;
pub mod defaults;
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
pub use stats::{compute_summaries, format_stats_table, generate_config_toml, MetricStats, PercentileSummary};
pub use stats_detailed::{collect_detailed_py, collect_detailed_rs, format_detailed_table, UnitMetrics};
pub use test_refs::{analyze_test_refs, is_test_file, CodeDefinition, TestRefAnalysis};
pub use units::{extract_code_units, CodeUnit, CodeUnitKind};

pub use rust_counts::analyze_rust_file;
pub use rust_fn_metrics::{
    compute_rust_file_metrics, compute_rust_function_metrics, RustFileMetrics,
    RustFunctionMetrics, RustTypeMetrics,
};
pub use rust_graph::build_rust_dependency_graph;
pub use rust_parsing::{parse_rust_file, parse_rust_files, ParsedRustFile, RustParseError};
pub use rust_test_refs::{
    analyze_rust_test_refs, is_rust_test_file, RustCodeDefinition, RustTestRefAnalysis,
};
pub use rust_units::{extract_rust_code_units, RustCodeUnit};

pub use rule_defs::{rules_for_python, rules_for_rust, Applicability, Rule, RuleCategory, RULES};
