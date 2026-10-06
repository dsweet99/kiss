use kiss::rpytest_runner::TestStatus;

use super::metrics::{LocalRubricMetrics, rust_cache_unstored};
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
    pub(crate) nextest_invocations: usize,
    pub(crate) discovered_python: usize,
    pub(crate) discovered_rust: usize,
    pub(crate) changed_source: usize,
    pub(crate) population_required: bool,
    pub(crate) prior_failures: usize,
    pub(crate) forced: usize,
    pub(crate) gate_reclassified: usize,
}

impl CacheDecisionMetrics {
    pub(crate) fn from_rubric(metrics: &LocalRubricMetrics) -> Self {
        let python_fresh = metrics.python.summary.cache_unstored;
        let observed = kiss::subprocess_observer::subprocess_observer_snapshot();
        Self {
            fresh_executions: rust_cache_unstored(metrics) + python_fresh,
            cache_hits: metrics.python.summary.cache_hits
                + metrics.rust_population.summary.cache_hits
                + metrics.rust_final.summary.cache_hits,
            pytest_invocations: prefer_observed(
                observed.pytest_invocations,
                one_if_positive(python_fresh),
            ),
            nextest_invocations: prefer_observed(
                observed.nextest_invocations,
                one_if_positive(rust_cache_unstored(metrics)),
            ),
            discovered_python: metrics.selected_python,
            discovered_rust: metrics.rust_final_selectors,
            changed_source: metrics.rust_vcs_source_paths,
            population_required: metrics.python_population_required
                || metrics.rust_population_required,
            prior_failures: metrics.prior_failures,
            forced: metrics.forced,
            gate_reclassified: gate_reclassified_count(&metrics.python.summary)
                + gate_reclassified_count(&metrics.rust_population.summary)
                + gate_reclassified_count(&metrics.rust_final.summary),
        }
    }

    pub(crate) fn print(&self) {
        println!("fresh_executions={}", self.fresh_executions);
        println!("cache_hits={}", self.cache_hits);
        println!("pytest_invocations={}", self.pytest_invocations);
        println!("nextest_invocations={}", self.nextest_invocations);
        println!("discovered_python={}", self.discovered_python);
        println!("discovered_rust={}", self.discovered_rust);
        println!("changed_source={}", self.changed_source);
        println!("population_required={}", self.population_required);
        println!("prior_failures={}", self.prior_failures);
        println!("forced={}", self.forced);
        println!("gate_reclassified={}", self.gate_reclassified);
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

#[cfg(test)]
mod tests {
    use super::CacheDecisionMetrics;

    #[test]
    fn observed_invocations_are_preferred_over_inferred_counts() {
        use std::sync::Arc;

        use kiss::subprocess_observer::{
            SubprocessObserver, SubprocessObserverSnapshot, bind_subprocess_observer,
            reset_subprocess_observer,
        };

        struct BusyObserver;
        impl SubprocessObserver for BusyObserver {
            fn record_pytest(&self) {}
            fn record_nextest(&self) {}
            fn snapshot(&self) -> SubprocessObserverSnapshot {
                SubprocessObserverSnapshot {
                    pytest_invocations: 2,
                    nextest_invocations: 3,
                }
            }
        }

        bind_subprocess_observer(Arc::new(BusyObserver));
        let metrics = CacheDecisionMetrics::from_rubric(&Default::default());
        reset_subprocess_observer();
        assert_eq!(metrics.pytest_invocations, 2);
        assert_eq!(metrics.nextest_invocations, 3);
        assert_eq!(metrics.fresh_executions, 0);
        assert!(!metrics.population_required);
        let idle = CacheDecisionMetrics::from_rubric(&Default::default());
        assert_eq!(idle.nextest_invocations, 0);
        assert_eq!(idle.gate_reclassified, 0);
    }
}
