use super::*;

use kiss::Language;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::test_runner::coverage_decision::LanguagePlanner;
use crate::test_runner::coverage_decision::{
    ChangedDiff, CoverageFreshness, LanguageTestModule, PopulationPlan, RunContext,
    SelectionDecision, TestSelector,
};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::runners::{python_backer, rust_backer};

fn planned() -> PlannedSelectors {
    PlannedSelectors {
        repo_root: PathBuf::from("."),
        py_sel: Vec::new(),
        rs_sel: Vec::new(),
        python_population_required: false,
        rust_population_required: false,
        rust_source_paths: Vec::new(),
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: Default::default(),
        ignore: Vec::new(),
    }
}

fn options(force_rerun: bool) -> SelectorRunOptions<'static> {
    SelectorRunOptions {
        dry_run: true,
        force_rerun,
        metrics: false,
        jobs: 1,
        extra: &[],
        plan_duration: Duration::ZERO,
    }
}

fn execution_module_rust(planned: &PlannedSelectors) -> rust_backer::RustModule {
    rust_backer::RustModule::for_execution(&planned.repo_root, &planned.ignore)
}

fn execution_module_python(planned: &PlannedSelectors) -> python_backer::PythonModule {
    python_backer::PythonModule::for_execution(&planned.repo_root, &planned.ignore)
}

struct FakeLanguageModule {
    language: Language,
    population_required: bool,
    selective: Vec<String>,
    summary: SelectorExecutionSummary,
}

impl LanguagePlanner for FakeLanguageModule {
    fn language(&self) -> Language {
        self.language
    }

    fn discover_universe(&self) -> Result<Vec<TestSelector>, String> {
        Ok(vec![TestSelector::new(
            self.language,
            format!("{:?}::population", self.language),
        )])
    }

    fn changed_tests(&self, _diff: &ChangedDiff) -> Vec<TestSelector> {
        Vec::new()
    }

    fn prior_failures(&self) -> Vec<TestSelector> {
        Vec::new()
    }

    fn freshness(&self, _universe: &[TestSelector]) -> Result<CoverageFreshness, String> {
        Ok(CoverageFreshness::Fresh)
    }

    fn population_plan(&self, universe: &[TestSelector]) -> PopulationPlan {
        PopulationPlan {
            selectors: universe.to_vec(),
        }
    }

    fn select(&self) -> Result<SelectionDecision, String> {
        Ok(SelectionDecision::default())
    }

    fn manifest_env_allowlist(&self) -> &'static [&'static str] {
        &[]
    }
}

impl crate::test_runner::coverage_decision::LanguageExecutor for FakeLanguageModule {
    fn language(&self) -> Language {
        self.language
    }

    fn population_required(&self, _ctx: &RunContext<'_, '_>) -> bool {
        self.population_required
    }

    fn selective_selectors(&self, _ctx: &RunContext<'_, '_>) -> Vec<String> {
        self.selective.clone()
    }

    fn run_population(
        &self,
        selectors: &[String],
        _ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        let mut summary = self.summary.clone();
        summary.total = selectors.len();
        Ok(summary)
    }

    fn run_selective(
        &self,
        selectors: &[String],
        _ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        let mut summary = self.summary.clone();
        summary.total = selectors.len();
        Ok(summary)
    }

    fn rebuild_index(&self, _ctx: &RunContext<'_, '_>) -> Result<(), String> {
        Ok(())
    }

    fn write_manifest(
        &self,
        selectors: &[String],
        _ctx: &RunContext<'_, '_>,
    ) -> Result<(), String> {
        let _ = (self.language, selectors.len());
        Ok(())
    }

    fn is_indexable_source(&self, _path: &Path, _repo_root: &Path) -> bool {
        true
    }

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        _extra: &[String],
        jobs: usize,
    ) -> Result<Vec<String>, String> {
        Ok(vec![format!(
            "{:?}:{population}:{jobs}:{}",
            self.language,
            selectors.join(",")
        )])
    }
}

mod part1;
mod part2;
