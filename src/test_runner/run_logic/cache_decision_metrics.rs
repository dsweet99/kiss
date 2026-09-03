use kiss::rpytest_runner::TestStatus;

use super::metrics::LocalRubricMetrics;
use super::metrics_rust::{
    rust_build_invocations, rust_cache_unstored, rust_current_index_generation, rust_export_jobs,
    rust_test_instances,
};
use super::runners::SelectorExecutionSummary;

macro_rules! println {
    ($($arg:tt)*) => {
        crate::test_runner::emit_test_progress(&format!($($arg)*))
    };
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CacheDecisionMetrics {
    pub(crate) fresh_executions: usize,
    pub(crate) cache_hits: usize,
    pub(crate) pytest_invocations: usize,
    pub(crate) cargo_invocations: usize,
    pub(crate) nextest_invocations: usize,
    pub(crate) llvm_export_invocations: usize,
    pub(crate) generation_merge_result: String,
    pub(crate) discovered_python: usize,
    pub(crate) discovered_rust: usize,
    pub(crate) changed_source: usize,
    pub(crate) population_required: bool,
    pub(crate) prior_failures: usize,
    pub(crate) forced: usize,
    pub(crate) gate_reclassified: usize,
    pub(crate) publication_generation_id: String,
    pub(crate) parent_generation_id: String,
}

impl CacheDecisionMetrics {
    pub(crate) fn from_rubric(metrics: &LocalRubricMetrics) -> Self {
        let python_fresh = metrics.python.summary.cache_unstored;
        let observed = kiss::rust_llvm_cov_runner::subprocess_observer_snapshot();
        Self {
            fresh_executions: rust_cache_unstored(metrics) + python_fresh,
            cache_hits: metrics.python.summary.cache_hits
                + metrics.rust_population.summary.cache_hits
                + metrics.rust_final.summary.cache_hits,
            pytest_invocations: prefer_observed(
                observed.pytest_invocations,
                one_if_positive(python_fresh),
            ),
            cargo_invocations: prefer_observed(
                observed.cargo_invocations,
                rust_build_invocations(metrics),
            ),
            nextest_invocations: prefer_observed(
                observed.nextest_invocations,
                one_if_positive(rust_test_instances(metrics)),
            ),
            llvm_export_invocations: prefer_observed(
                observed.llvm_export_invocations,
                rust_export_jobs(metrics),
            ),
            generation_merge_result: rust_current_index_generation(metrics).to_string(),
            discovered_python: metrics.selected_python,
            discovered_rust: metrics.rust_final_selectors,
            changed_source: metrics.rust_snapshot_delta_modified,
            population_required: metrics.python_population_required
                || metrics.rust_population_required,
            prior_failures: metrics.prior_failures,
            forced: metrics.forced,
            gate_reclassified: gate_reclassified_count(&metrics.python.summary)
                + gate_reclassified_count(&metrics.rust_population.summary)
                + gate_reclassified_count(&metrics.rust_final.summary),
            publication_generation_id: publication_generation_id(metrics),
            parent_generation_id: metrics.parent_generation_id.clone(),
        }
    }

    pub(crate) fn print(&self) {
        println!("fresh_executions={}", self.fresh_executions);
        println!("cache_hits={}", self.cache_hits);
        println!("pytest_invocations={}", self.pytest_invocations);
        println!("cargo_invocations={}", self.cargo_invocations);
        println!("nextest_invocations={}", self.nextest_invocations);
        println!("llvm_export_invocations={}", self.llvm_export_invocations);
        println!("generation_merge_result={}", self.generation_merge_result);
        println!("discovered_python={}", self.discovered_python);
        println!("discovered_rust={}", self.discovered_rust);
        println!("changed_source={}", self.changed_source);
        println!("population_required={}", self.population_required);
        println!("prior_failures={}", self.prior_failures);
        println!("forced={}", self.forced);
        println!("gate_reclassified={}", self.gate_reclassified);
        println!(
            "publication_generation_id={}",
            self.publication_generation_id
        );
        println!("parent_generation_id={}", self.parent_generation_id);
    }
}

fn one_if_positive(count: usize) -> usize {
    if count > 0 { 1 } else { 0 }
}

fn prefer_observed(observed: usize, inferred: usize) -> usize {
    if observed > 0 { observed } else { inferred }
}

fn gate_reclassified_count(summary: &SelectorExecutionSummary) -> usize {
    summary
        .raw_statuses
        .iter()
        .filter(|(sel, raw)| {
            **raw == TestStatus::Passed && summary.timed_out_selectors.iter().any(|s| s == *sel)
        })
        .count()
}

fn publication_generation_id(metrics: &LocalRubricMetrics) -> String {
    let from_run = rust_current_index_generation(metrics);
    if from_run.is_empty() {
        metrics.publication_generation_id.clone()
    } else {
        from_run.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::CacheDecisionMetrics;

    #[test]
    fn default_metrics_print_zero_counts() {
        use std::sync::Arc;

        use kiss::rust_llvm_cov_runner::{
            SubprocessObserver, SubprocessObserverSnapshot, bind_subprocess_observer,
        };

        struct IdleObserver;
        impl SubprocessObserver for IdleObserver {
            fn record_pytest(&self) {}
            fn record_cargo_nextest(&self) {}
            fn record_llvm_export(&self, _: usize) {}
            fn snapshot(&self) -> SubprocessObserverSnapshot {
                SubprocessObserverSnapshot::default()
            }
        }

        kiss::rust_llvm_cov_runner::reset_subprocess_observer();
        bind_subprocess_observer(Arc::new(IdleObserver));
        let observed = kiss::rust_llvm_cov_runner::subprocess_observer_snapshot();
        assert_eq!(observed.pytest_invocations, 0);
        assert_eq!(observed.cargo_invocations, 0);
        kiss::rust_llvm_cov_runner::reset_subprocess_observer();
        let metrics = CacheDecisionMetrics::default();
        assert_eq!(metrics.fresh_executions, 0);
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.generation_merge_result, "");
        assert_eq!(metrics.discovered_python, 0);
        assert_eq!(metrics.discovered_rust, 0);
        assert!(!metrics.population_required);
        assert_eq!(metrics.prior_failures, 0);
        assert_eq!(metrics.forced, 0);
        assert_eq!(metrics.gate_reclassified, 0);
        assert_eq!(metrics.publication_generation_id, "");
        assert_eq!(metrics.parent_generation_id, "");
    }
}
