use std::fs;
use std::path::Path;
use std::time::Duration;

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
    pub(super) rust_population_required: bool,
    pub(super) rust_population_selectors: usize,
    pub(super) rust_final_selectors: usize,
    pub(super) selection_basis: crate::test_runner::test_selection::SelectionBasis,
    pub(super) selection_engine_used: bool,
    pub(super) python: PhaseMetrics,
    pub(super) python_index_rebuild_duration: Duration,
    pub(super) rust_population: PhaseMetrics,
    pub(super) rust_index_rebuild_duration: Duration,
    pub(super) rust_final: PhaseMetrics,
    pub(super) kiss_cache_residual_bytes: u64,
    pub(super) rust_concurrency_budget: usize,
    pub(super) exit_code: i32,
    pub(super) prior_failures: usize,
    pub(super) forced: usize,
}

impl LocalRubricMetrics {
    pub(super) fn new(
        planned: &PlannedSelectors,
        options: &SelectorRunOptions<'_>,
        python_population_selectors: usize,
        rust_population_required: bool,
        rust_population_selectors: usize,
        rust_final_selectors: usize,
        selection_basis: crate::test_runner::test_selection::SelectionBasis,
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
            rust_population_required,
            rust_population_selectors,
            rust_final_selectors,
            selection_basis,
            selection_engine_used: planned.selection_engine_used,
            python: PhaseMetrics::default(),
            python_index_rebuild_duration: Duration::ZERO,
            rust_population: PhaseMetrics::default(),
            rust_index_rebuild_duration: Duration::ZERO,
            rust_final: PhaseMetrics::default(),
            kiss_cache_residual_bytes: 0,
            rust_concurrency_budget: options.jobs,
            exit_code: 0,
            prior_failures: planned.prior_failure_selectors.python.len()
                + planned.prior_failure_selectors.rust.len(),
            forced: forced_selector_count(planned, options),
        }
    }

    pub(super) fn stage_mut(&mut self, stage: &str) -> (&mut PhaseMetrics, &mut Duration) {
        match stage {
            "python" => (&mut self.python, &mut self.python_index_rebuild_duration),
            "rust_population" => (
                &mut self.rust_population,
                &mut self.rust_index_rebuild_duration,
            ),
            "rust_final" => (&mut self.rust_final, &mut self.rust_index_rebuild_duration),
            other => panic!("no metrics slot for stage `{other}`"),
        }
    }

    pub(super) fn capture_cache_shape(&mut self, repo_root: &Path) {
        self.kiss_cache_residual_bytes = path_size_bytes(&repo_root.join(".kiss"));
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
    println!("selection_engine_used={}", metrics.selection_engine_used);
}

fn selection_basis_label(
    basis: crate::test_runner::test_selection::SelectionBasis,
) -> &'static str {
    use crate::test_runner::test_selection::SelectionBasis;
    match basis {
        SelectionBasis::Current => "current",
        SelectionBasis::Population => "population",
    }
}

#[cfg(test)]
mod basis_label_tests {
    use super::selection_basis_label;
    use crate::test_runner::test_selection::SelectionBasis;

    #[test]
    fn selection_basis_metrics_label_all_planning_modes() {
        assert_eq!(selection_basis_label(SelectionBasis::Current), "current");
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
        "rust_concurrency_budget={}",
        metrics.rust_concurrency_budget
    );
    println!("rust_cache_unstored={}", rust_cache_unstored(metrics));
    super::cache_decision_metrics::CacheDecisionMetrics::from_rubric(metrics).print();
}

pub(super) fn rust_cache_unstored(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.cache_unstored + metrics.rust_final.summary.cache_unstored
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

fn forced_selector_count(planned: &PlannedSelectors, options: &SelectorRunOptions<'_>) -> usize {
    if options.force_rerun {
        planned.sel.python.len() + planned.sel.rust.len()
    } else {
        planned.prior_failure_selectors.python.len() + planned.prior_failure_selectors.rust.len()
    }
}

#[cfg(test)]
#[path = "metrics_test.rs"]
mod tests;
