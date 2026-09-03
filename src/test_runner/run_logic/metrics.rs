use std::fs;
use std::path::Path;
use std::time::Duration;

use kiss::rust_llvm_cov_runner::rust_cov_cache_tmp_parent;

use super::metrics_rust::{print_rust_batch_metrics, rust_cache_unstored};
use super::runners::SelectorExecutionSummary;
use super::{PlannedSelectors, SelectorRunOptions};

macro_rules! println {
    ($($arg:tt)*) => {
        crate::test_runner::emit_test_progress(&format!($($arg)*))
    };
}

#[derive(Default)]
pub(super) struct PhaseMetrics {
    pub(super) duration: Duration,
    pub(super) summary: SelectorExecutionSummary,
}

#[derive(Default)]
pub(super) struct LocalRubricMetrics {
    pub(super) plan_duration: Duration,
    pub(super) total_duration: Duration,
    pub(super) selected_python: usize,
    pub(super) python_population_required: bool,
    pub(super) python_population_selectors: usize,
    pub(super) selected_rust_initial: usize,
    pub(super) rust_source_paths: usize,
    pub(super) rust_vcs_source_paths: usize,
    pub(super) rust_snapshot_delta_modified: usize,
    pub(super) rust_snapshot_delta_structural: bool,
    pub(super) rust_population_required: bool,
    pub(super) rust_population_selectors: usize,
    pub(super) rust_final_selectors: usize,
    pub(super) selection_basis: crate::test_runner::coverage_decision::SelectionBasis,
    pub(super) coverage_decision_engine_used: bool,
    pub(super) python: PhaseMetrics,
    pub(super) python_index_rebuild_duration: Duration,
    pub(super) rust_population: PhaseMetrics,
    pub(super) rust_index_rebuild_duration: Duration,
    pub(super) rust_final: PhaseMetrics,
    pub(super) kiss_cache_residual_bytes: u64,
    pub(super) rust_cache_residual_bytes: u64,
    pub(super) rust_entry_cache_bytes: u64,
    pub(super) rust_build_target_bytes: u64,
    pub(super) rust_build_target_baseline_bytes: u64,
    pub(super) raw_artifact_count: usize,
    pub(super) rust_build_target_count: usize,
    pub(super) rust_transient_residual_count: usize,
    pub(super) rust_external_tmp_residual_bytes: u64,
    pub(super) rust_external_tmp_residual_count: usize,
    pub(super) rust_external_tmp_metric_error: bool,
    pub(super) rust_concurrency_budget: usize,
    pub(super) exit_code: i32,
    pub(super) prior_failures: usize,
    pub(super) forced: usize,
    pub(super) publication_generation_id: String,
    pub(super) parent_generation_id: String,
}

impl LocalRubricMetrics {
    pub(super) fn new(
        planned: &PlannedSelectors,
        options: &SelectorRunOptions<'_>,
        python_population_selectors: usize,
        rust_population_required: bool,
        rust_population_selectors: usize,
        rust_final_selectors: usize,
        selection_basis: crate::test_runner::coverage_decision::SelectionBasis,
    ) -> Self {
        Self {
            plan_duration: options.plan_duration,
            total_duration: Duration::ZERO,
            selected_python: planned.sel.python.len(),
            python_population_required: planned.population_required.python,
            python_population_selectors,
            selected_rust_initial: planned.sel.rust.len(),
            rust_source_paths: planned.source_paths.rust.len(),
            rust_vcs_source_paths: planned.vcs_source_paths.rust,
            rust_snapshot_delta_modified: planned.snapshot_delta_modified.rust,
            rust_snapshot_delta_structural: planned.snapshot_delta_structural.rust,
            rust_population_required,
            rust_population_selectors,
            rust_final_selectors,
            selection_basis,
            coverage_decision_engine_used: planned.coverage_decision_engine_used,
            python: PhaseMetrics::default(),
            python_index_rebuild_duration: Duration::ZERO,
            rust_population: PhaseMetrics::default(),
            rust_index_rebuild_duration: Duration::ZERO,
            rust_final: PhaseMetrics::default(),
            kiss_cache_residual_bytes: 0,
            rust_cache_residual_bytes: 0,
            rust_entry_cache_bytes: 0,
            rust_build_target_bytes: 0,
            rust_build_target_baseline_bytes: 0,
            raw_artifact_count: 0,
            rust_build_target_count: 0,
            rust_transient_residual_count: 0,
            rust_external_tmp_residual_bytes: 0,
            rust_external_tmp_residual_count: 0,
            rust_external_tmp_metric_error: false,
            rust_concurrency_budget: options.jobs,
            exit_code: 0,
            prior_failures: planned.prior_failure_selectors.python.len()
                + planned.prior_failure_selectors.rust.len(),
            forced: forced_selector_count(planned, options),
            publication_generation_id: String::new(),
            parent_generation_id: String::new(),
        }
    }

    pub(super) fn capture_cache_shape(&mut self, repo_root: &Path) {
        let kiss_cache = repo_root.join(".kiss");
        let rust_cache = kiss_cache.join("rust_llvm_cov_cache");
        self.kiss_cache_residual_bytes = path_size_bytes(&kiss_cache);
        self.rust_cache_residual_bytes = path_size_bytes(&rust_cache);
        self.rust_entry_cache_bytes = path_size_bytes(&rust_cache.join("entries"));
        self.rust_build_target_bytes = path_size_bytes(&rust_cache.join("build").join("target"));
        self.raw_artifact_count = count_json_files(&rust_cache.join("artifacts"));
        self.rust_build_target_count = count_build_targets(&rust_cache.join("build"));
        match path_size_and_count(&rust_cov_cache_tmp_parent(&rust_cache)) {
            Ok((bytes, count)) => {
                self.rust_external_tmp_residual_bytes = bytes;
                self.rust_external_tmp_residual_count = count;
                self.rust_external_tmp_metric_error = false;
                self.rust_transient_residual_count = self.raw_artifact_count + count;
            }
            Err(_) => {
                self.rust_external_tmp_residual_bytes = u64::MAX;
                self.rust_external_tmp_residual_count = usize::MAX;
                self.rust_external_tmp_metric_error = true;
                self.rust_transient_residual_count = usize::MAX;
            }
        }
        let (publication, parent) = generation_pointer_ids(&rust_cache);
        self.publication_generation_id = publication;
        self.parent_generation_id = parent;
    }

    pub(super) fn print(&self) {
        print_oracle_metrics();
        print_selection_metrics(self);
        print_phase_metrics("python", &self.python);
        print_phase_metrics("rust_population", &self.rust_population);
        print_phase_metrics("rust_final", &self.rust_final);
        print_timing_metrics(self);
        print_cache_metrics(self);
        println!("exit_code={}", self.exit_code);
    }
}

fn print_oracle_metrics() {
    println!("KISS TEST METRICS");
    println!("oracle_selector_recall=external_required");
    println!("oracle_false_negative_rate=external_required");
    println!("oracle_exit_code_agreement=external_required");
    println!("selection_ratio=external_full_suite_count_required");
    println!("time_saved_ratio=external_full_suite_time_required");
}

fn print_selection_metrics(metrics: &LocalRubricMetrics) {
    println!("selected_python={}", metrics.selected_python);
    println!(
        "python_population_required={}",
        metrics.python_population_required
    );
    println!(
        "python_population_selectors={}",
        metrics.python_population_selectors
    );
    println!("selected_rust_initial={}", metrics.selected_rust_initial);
    println!("rust_vcs_source_paths={}", metrics.rust_vcs_source_paths);
    println!("rust_source_paths={}", metrics.rust_source_paths);
    println!(
        "rust_snapshot_delta_modified={}",
        metrics.rust_snapshot_delta_modified
    );
    println!(
        "rust_snapshot_delta_structural={}",
        metrics.rust_snapshot_delta_structural
    );
    println!(
        "rust_population_required={}",
        metrics.rust_population_required
    );
    println!(
        "rust_population_selectors={}",
        metrics.rust_population_selectors
    );
    println!("rust_final_selectors={}", metrics.rust_final_selectors);
    println!(
        "selection_basis={}",
        selection_basis_label(metrics.selection_basis)
    );
    println!(
        "coverage_decision_engine_used={}",
        metrics.coverage_decision_engine_used
    );
}

fn selection_basis_label(
    basis: crate::test_runner::coverage_decision::SelectionBasis,
) -> &'static str {
    use crate::test_runner::coverage_decision::SelectionBasis;
    match basis {
        SelectionBasis::Current => "current",
        SelectionBasis::ReusablePrior => "reusable_prior",
        SelectionBasis::Population => "population",
    }
}

#[cfg(test)]
mod basis_label_tests {
    use super::selection_basis_label;
    use crate::test_runner::coverage_decision::SelectionBasis;

    #[test]
    fn selection_basis_metrics_label_all_planning_modes() {
        assert_eq!(selection_basis_label(SelectionBasis::Current), "current");
        assert_eq!(
            selection_basis_label(SelectionBasis::ReusablePrior),
            "reusable_prior"
        );
        assert_eq!(
            selection_basis_label(SelectionBasis::Population),
            "population"
        );
    }
}

fn print_timing_metrics(metrics: &LocalRubricMetrics) {
    println!("phase_plan_ms={}", metrics.plan_duration.as_millis());
    println!("phase_python_ms={}", metrics.python.duration.as_millis());
    println!(
        "phase_python_index_rebuild_ms={}",
        metrics.python_index_rebuild_duration.as_millis()
    );
    println!(
        "phase_rust_population_ms={}",
        metrics.rust_population.duration.as_millis()
    );
    println!(
        "phase_rust_index_rebuild_ms={}",
        metrics.rust_index_rebuild_duration.as_millis()
    );
    println!(
        "phase_rust_final_ms={}",
        metrics.rust_final.duration.as_millis()
    );
    println!("phase_total_ms={}", metrics.total_duration.as_millis());
}

fn print_cache_metrics(metrics: &LocalRubricMetrics) {
    println!(
        "kiss_cache_residual_bytes={}",
        metrics.kiss_cache_residual_bytes
    );
    println!(
        "rust_cache_residual_bytes={}",
        metrics.rust_cache_residual_bytes
    );
    println!("rust_entry_cache_bytes={}", metrics.rust_entry_cache_bytes);
    println!(
        "rust_concurrency_budget={}",
        metrics.rust_concurrency_budget
    );
    print_rust_batch_metrics(metrics);
    println!("rust_cache_unstored={}", rust_cache_unstored(metrics));
    super::cache_decision_metrics::CacheDecisionMetrics::from_rubric(metrics).print();
    println!("raw_artifact_count={}", metrics.raw_artifact_count);
    println!(
        "rust_external_tmp_residual_bytes={}",
        metrics.rust_external_tmp_residual_bytes
    );
    println!(
        "rust_external_tmp_residual_count={}",
        metrics.rust_external_tmp_residual_count
    );
    println!(
        "rust_transient_residual_count={}",
        metrics.rust_transient_residual_count
    );
    println!(
        "raw_artifact_residuals_pass={}",
        metrics.raw_artifact_count == 0
    );
    println!(
        "rust_build_target_bound_pass={}",
        metrics.rust_build_target_count <= 1
    );
    println!(
        "rust_external_tmp_residuals_pass={}",
        !metrics.rust_external_tmp_metric_error
            && metrics.rust_external_tmp_residual_bytes == 0
            && metrics.rust_external_tmp_residual_count == 0
    );
}

fn print_phase_metrics(name: &str, phase: &PhaseMetrics) {
    println!("{name}_total={}", phase.summary.total);
    println!("{name}_cache_hits={}", phase.summary.cache_hits);
    println!("{name}_cache_misses={}", phase.summary.cache_misses);
    println!("{name}_cache_unstored={}", phase.summary.cache_unstored);
    println!("{name}_failed={}", phase.summary.failed);
}

fn path_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| path_size_bytes(&entry.path()))
        .sum()
}

fn path_size_and_count(path: &Path) -> std::io::Result<(u64, usize)> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(err) => return Err(err),
    };
    if meta.is_file() {
        return Ok((meta.len(), 1));
    }
    if !meta.is_dir() {
        return Ok((0, 1));
    }
    let mut bytes = 0;
    let mut count = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let (child_bytes, child_count) = path_size_and_count(&entry.path())?;
        bytes += child_bytes;
        count += child_count;
    }
    Ok((bytes, count))
}

fn count_json_files(path: &Path) -> usize {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .count()
}

fn count_build_targets(path: &Path) -> usize {
    usize::from(path.join("target").is_dir())
}

fn forced_selector_count(planned: &PlannedSelectors, options: &SelectorRunOptions<'_>) -> usize {
    if options.force_rerun {
        planned.sel.python.len() + planned.sel.rust.len()
    } else {
        planned.prior_failure_selectors.python.len() + planned.prior_failure_selectors.rust.len()
    }
}

fn generation_pointer_ids(rust_cache: &Path) -> (String, String) {
    crate::test_runner::execution_generation::read_pointer(rust_cache)
        .ok()
        .flatten()
        .map(|pointer| (pointer.generation_id, pointer.parent_generation_id))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "metrics_test.rs"]
mod tests;
