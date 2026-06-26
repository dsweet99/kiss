use std::collections::BTreeSet;

use super::backer::CoverageBacker;
use super::types::{ChangedDiff, ChangedSource, CoverageDecisionPlan};

pub(crate) struct CoverageDecisionEngine {
    backers: Vec<CoverageBacker>,
}

impl CoverageDecisionEngine {
    pub(crate) fn new(backers: Vec<CoverageBacker>) -> Self {
        Self { backers }
    }

    pub(crate) fn plan(
        &self,
        changed_sources: &[ChangedSource],
    ) -> Result<CoverageDecisionPlan, String> {
        let diff = ChangedDiff::new(changed_sources.to_vec());
        let mut selected = BTreeSet::new();
        let mut population = BTreeSet::new();
        let mut population_languages = Vec::new();
        for backer in &self.backers {
            let universe = backer.discover_test_universe()?;
            let changed_tests = backer.changed_tests(&diff);
            let prior_failures = backer.prior_failures();
            let freshness = backer.freshness(&universe)?;
            if freshness.requires_population() {
                if !population_languages.contains(&backer.language()) {
                    population_languages.push(backer.language());
                }
                population.extend(backer.population_plan(&universe).selectors);
                population.extend(changed_tests);
                population.extend(prior_failures);
            } else {
                let language_sources = diff.sources_for_language(backer.language());
                selected.extend(backer.select(&language_sources)?.selectors);
                selected.extend(changed_tests);
                selected.extend(prior_failures);
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
