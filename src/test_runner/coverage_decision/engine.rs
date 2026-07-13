use std::collections::BTreeSet;

use super::language_module::LanguagePlanner;
use super::types::{ChangedDiff, ChangedSource, CoverageDecisionPlan};

pub(crate) struct CoverageDecisionEngine {
    planners: Vec<Box<dyn LanguagePlanner>>,
}

impl CoverageDecisionEngine {
    pub(crate) fn new(planners: Vec<Box<dyn LanguagePlanner>>) -> Self {
        Self { planners }
    }

    pub(crate) fn plan(
        &self,
        changed_sources: &[ChangedSource],
    ) -> Result<CoverageDecisionPlan, String> {
        let diff = ChangedDiff::new(changed_sources.to_vec());
        let mut selected = BTreeSet::new();
        let mut population = BTreeSet::new();
        let mut population_languages = Vec::new();
        for planner in &self.planners {
            let universe = planner.discover_universe()?;
            let universe_ids = universe
                .iter()
                .map(|selector| selector.id.clone())
                .collect::<BTreeSet<_>>();
            let changed_tests = planner.changed_tests(&diff);
            let prior_failures =
                filter_selectors_to_universe(planner.prior_failures(), &universe_ids);
            let freshness = planner.freshness(&universe)?;
            if freshness.requires_population() {
                if !population_languages.contains(&planner.language()) {
                    population_languages.push(planner.language());
                }
                population.extend(planner.population_plan(&universe).selectors);
                population.extend(changed_tests);
                population.extend(prior_failures);
            } else {
                let decision = planner.select()?;
                if decision.complete {
                    selected.extend(decision.selectors);
                    selected.extend(changed_tests);
                    selected.extend(prior_failures);
                } else {
                    if !population_languages.contains(&planner.language()) {
                        population_languages.push(planner.language());
                    }
                    population.extend(planner.population_plan(&universe).selectors);
                    population.extend(changed_tests);
                    population.extend(prior_failures);
                }
            }
        }
        selected.retain(|selector| !population.contains(selector));
        Ok(CoverageDecisionPlan {
            selected: selected.into_iter().collect(),
            population: population.into_iter().collect(),
            population_languages,
        })
    }
}

fn filter_selectors_to_universe(
    selectors: Vec<super::types::TestSelector>,
    universe_ids: &BTreeSet<String>,
) -> Vec<super::types::TestSelector> {
    selectors
        .into_iter()
        .filter(|selector| universe_ids.contains(&selector.id))
        .collect()
}
