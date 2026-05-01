//! Full-universe analysis: parsing, graphs, coverage, duplication, and reporting.

mod cache;
mod coverage_types;
mod coverage;
mod coverage_gate;
mod dup_detect;
mod dry;
mod entry;
mod finalize_types;
mod finalize;
mod focus;
mod gated;
mod graph_api;
mod metrics_global;
mod options;
mod params;
mod parallel;
mod pipeline;
mod print;

// `pub use` items are re-exports for `crate::analyze::*`; the RHS is otherwise unused in this module.
#[allow(unused_imports)]
pub use coverage::compute_test_coverage_from_lists;
#[allow(unused_imports)] // Public API surface (`crate::analyze::check_coverage_gate`).
pub use coverage_gate::check_coverage_gate;
#[allow(unused_imports)] // Public API surface for cached-coverage-gate checks.
pub(crate) use coverage_gate::evaluate_cached_gate;
pub use dry::{run_dry, DryRunParams};
#[allow(unused_imports)] // Public API surface for `kiss` library consumers.
pub use dup_detect::{detect_py_duplicates, detect_rs_duplicates};
pub use entry::{run_analyze, run_analyze_with_result};
#[allow(unused_imports)]
pub(crate) use cache::{FullCacheStoreInput, maybe_store_full_cache};
pub use focus::{
    build_focus_set, filter_duplicates_by_focus, filter_viols_by_focus, gather_files, is_focus_file,
};
#[allow(unused_imports)]
pub use graph_api::{
    analyze_graphs, build_graphs, build_py_graph_from_files, build_rs_graph_from_files, graph_for_path,
    AnalyzeGraphsIn, GraphConfigs,
};
#[allow(unused_imports)]
pub use coverage_types::CheckCoverageGateParams;
pub use metrics_global::{compute_global_metrics, GlobalMetricsInput};
pub use options::{AnalyzeOptions, AnalyzeResult};
#[allow(unused_imports)]
pub(crate) use pipeline::{FullPipelineInput, FullPipelineResult, run_full_pipeline};
#[cfg(test)]
mod tests_smoke;
#[cfg(test)]
mod tests_coverage;
#[cfg(test)]
mod tests_touch;
