use std::collections::BTreeSet;

use super::language_module::LanguagePlanner;
use super::types::{ChangedDiff, ChangedSource, CoverageDecisionPlan, TestSelector};

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
        let plan_trace = std::env::var_os("KISS_PLAN_TRACE").is_some();
        for planner in &self.planners {
            let mark = std::time::Instant::now();
            let changed_tests = planner.changed_tests(&diff);
            let (selected_part, population_part, needs_population) =
                plan_language(planner.as_ref(), &diff, changed_tests)?;
            if plan_trace {
                eprintln!(
                    "KISS_PLAN_TRACE engine_{:?}_ms={}",
                    planner.language(),
                    mark.elapsed().as_millis()
                );
            }
            if needs_population {
                if !population_languages.contains(&planner.language()) {
                    population_languages.push(planner.language());
                }
                population.extend(population_part);
            } else {
                selected.extend(selected_part);
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

fn plan_language(
    planner: &dyn LanguagePlanner,
    _diff: &ChangedDiff,
    changed_tests: Vec<TestSelector>,
) -> Result<(BTreeSet<TestSelector>, BTreeSet<TestSelector>, bool), String> {
    match planner.language() {
        kiss::Language::Rust => plan_rust_language(planner, changed_tests),
        kiss::Language::Python => plan_python_language(planner, changed_tests),
    }
}

fn plan_python_language(
    planner: &dyn LanguagePlanner,
    changed_tests: Vec<TestSelector>,
) -> Result<(BTreeSet<TestSelector>, BTreeSet<TestSelector>, bool), String> {
    plan_selective_or_population(planner, changed_tests)
}

fn plan_rust_language(
    planner: &dyn LanguagePlanner,
    changed_tests: Vec<TestSelector>,
) -> Result<(BTreeSet<TestSelector>, BTreeSet<TestSelector>, bool), String> {
    plan_selective_or_population(planner, changed_tests)
}

fn plan_selective_or_population(
    planner: &dyn LanguagePlanner,
    changed_tests: Vec<TestSelector>,
) -> Result<(BTreeSet<TestSelector>, BTreeSet<TestSelector>, bool), String> {
    let plan_trace = std::env::var_os("KISS_PLAN_TRACE").is_some();
    let mut mark = std::time::Instant::now();
    let freshness = planner.freshness(&[])?;
    if plan_trace {
        eprintln!(
            "KISS_PLAN_TRACE rust_freshness_ms={} requires_pop={}",
            mark.elapsed().as_millis(),
            freshness.requires_population()
        );
        mark = std::time::Instant::now();
    }
    if freshness.requires_population() {
        return plan_population(planner, changed_tests);
    }
    let decision = planner.select()?;
    if plan_trace {
        eprintln!(
            "KISS_PLAN_TRACE rust_select_ms={} complete={} selected={}",
            mark.elapsed().as_millis(),
            decision.complete,
            decision.selectors.len()
        );
        mark = std::time::Instant::now();
    }
    if !decision.complete {
        return plan_population(planner, changed_tests);
    }
    let mut prior_failures = planner.prior_failures();
    if !prior_failures.is_empty() {
        let universe_ids = planner
            .discover_universe()?
            .into_iter()
            .map(|selector| selector.id)
            .collect::<BTreeSet<_>>();
        prior_failures = filter_selectors_to_universe(prior_failures, &universe_ids);
    }
    let mut selected = BTreeSet::new();
    selected.extend(decision.selectors);
    selected.extend(changed_tests);
    selected.extend(prior_failures);
    if plan_trace {
        eprintln!(
            "KISS_PLAN_TRACE rust_assemble_ms={} total_selected={}",
            mark.elapsed().as_millis(),
            selected.len()
        );
    }
    Ok((selected, BTreeSet::new(), false))
}

fn plan_population(
    planner: &dyn LanguagePlanner,
    changed_tests: Vec<TestSelector>,
) -> Result<(BTreeSet<TestSelector>, BTreeSet<TestSelector>, bool), String> {
    let universe = planner.discover_universe()?;
    let universe_ids = universe
        .iter()
        .map(|selector| selector.id.clone())
        .collect::<BTreeSet<_>>();
    let prior_failures = filter_selectors_to_universe(planner.prior_failures(), &universe_ids);
    let mut population = planner.population_plan(&universe).selectors;
    population.extend(changed_tests);
    population.extend(prior_failures);
    Ok((BTreeSet::new(), population.into_iter().collect(), true))
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
