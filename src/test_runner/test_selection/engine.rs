use std::collections::BTreeSet;

use super::language_module::LanguagePlanner;
use super::types::{ChangedDiff, ChangedSource, SelectionPlan, TestSelector};

pub(crate) struct SelectionEngine {
    planners: Vec<Box<dyn LanguagePlanner>>,
}

impl SelectionEngine {
    pub(crate) fn new(planners: Vec<Box<dyn LanguagePlanner>>) -> Self {
        Self { planners }
    }

    pub(crate) fn plan(&self, changed_sources: &[ChangedSource]) -> Result<SelectionPlan, String> {
        let diff = ChangedDiff::new(changed_sources.to_vec());
        let mut selected = BTreeSet::new();
        let mut population = BTreeSet::new();
        let mut population_languages = Vec::new();
        let plan_trace = std::env::var_os("KISS_PLAN_TRACE").is_some();
        for planner in &self.planners {
            let mark = std::time::Instant::now();
            let changed_tests = planner.changed_tests(&diff);
            if planner.sources_changed() {
                if !population_languages.contains(&planner.language()) {
                    population_languages.push(planner.language());
                }
                population.extend(plan_population(planner.as_ref(), changed_tests)?);
            } else {
                selected.extend(plan_selective(planner.as_ref(), changed_tests)?);
            }
            if plan_trace {
                eprintln!(
                    "KISS_PLAN_TRACE engine_{:?}_ms={}",
                    planner.language(),
                    mark.elapsed().as_millis()
                );
            }
        }
        selected.retain(|selector| !population.contains(selector));
        Ok(SelectionPlan {
            selected: selected.into_iter().collect(),
            population: population.into_iter().collect(),
            population_languages,
        })
    }
}

/// The changed tests and the prior failures that still exist.
fn plan_selective(
    planner: &dyn LanguagePlanner,
    changed_tests: Vec<TestSelector>,
) -> Result<BTreeSet<TestSelector>, String> {
    let mut selected: BTreeSet<TestSelector> = changed_tests.into_iter().collect();
    let prior_failures = planner.prior_failures();
    if !prior_failures.is_empty() {
        let universe_ids = universe_ids(planner)?;
        selected.extend(filter_selectors_to_universe(prior_failures, &universe_ids));
    }
    Ok(selected)
}

/// Every test of the language, plus the changed tests and the prior failures that still exist.
fn plan_population(
    planner: &dyn LanguagePlanner,
    changed_tests: Vec<TestSelector>,
) -> Result<BTreeSet<TestSelector>, String> {
    let universe = planner.discover_universe()?;
    let universe_ids = universe
        .iter()
        .map(|selector| selector.id.clone())
        .collect::<BTreeSet<_>>();
    let prior_failures = filter_selectors_to_universe(planner.prior_failures(), &universe_ids);
    let mut population: BTreeSet<TestSelector> = universe.into_iter().collect();
    population.extend(changed_tests);
    population.extend(prior_failures);
    Ok(population)
}

fn universe_ids(planner: &dyn LanguagePlanner) -> Result<BTreeSet<String>, String> {
    Ok(planner
        .discover_universe()?
        .into_iter()
        .map(|selector| selector.id)
        .collect())
}

fn filter_selectors_to_universe(
    selectors: Vec<TestSelector>,
    universe_ids: &BTreeSet<String>,
) -> Vec<TestSelector> {
    selectors
        .into_iter()
        .filter(|selector| universe_ids.contains(&selector.id))
        .collect()
}
