mod cache;
#[cfg(test)]
pub(crate) mod cov_cache_test_support;
pub(crate) mod cov_file_list_cache;
pub(crate) mod cov_records_cache;
mod coverage;
mod coverage_gate;
mod coverage_types;
mod dry;
mod dup_detect;
mod entry;
mod finalize;
mod finalize_types;
mod focus;
mod graph_api;
mod lang_sides;
pub(crate) mod line_coverage;
mod orphan_unit_gate;
mod options;
mod parallel;
mod params;
mod pipeline;
mod print;

#[allow(unused_imports)]
pub(crate) use cache::{FullCacheStoreInput, maybe_store_full_cache};
pub(crate) use coverage::collect_line_coverage_viols;
#[allow(unused_imports)]
pub use coverage_gate::check_coverage_gate;
pub(crate) use coverage_gate::evaluate_line_gate;
pub(crate) use orphan_unit_gate::evaluate_orphan_unit_gate;
#[allow(unused_imports)]
pub use coverage_types::CheckCoverageGateParams;
pub use dry::{DryRunParams, run_dry};
#[allow(unused_imports)]
pub use dup_detect::{detect_py_duplicates, detect_rs_duplicates};
#[allow(unused_imports)]
pub use entry::{run_analyze, run_analyze_with_result};
#[allow(unused_imports)]
pub use focus::{
    FocusFilter, build_focus_filter, build_focus_set, filter_duplicates_by_focus,
    filter_viols_by_focus, gather_files, is_focus_file,
};
#[allow(unused_imports)]
pub use graph_api::{
    AnalyzeGraphsIn, GraphConfigs, analyze_graphs, build_graphs, build_py_graph_from_files,
    build_rs_graph_from_files, graph_for_path,
};
#[allow(unused_imports)]
pub use options::{AnalyzeOptions, AnalyzeResult};
#[cfg(test)]
pub(crate) use pipeline::empty_full_pipeline_result_for_tests;
#[allow(unused_imports)]
pub(crate) use pipeline::{FullPipelineInput, FullPipelineResult, run_full_pipeline};
#[cfg(test)]
mod tests_coverage;
#[cfg(test)]
mod tests_smoke;
