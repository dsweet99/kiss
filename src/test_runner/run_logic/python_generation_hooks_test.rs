use super::{generation_already_current, is_indexable};
use crate::test_runner::coverage_decision::RunContext;
use crate::test_runner::python_coverage_index::generation::{
    PopulationEvidence, SelectorEvidence, TimingCacheDisposition, population_plan_for_selectors,
    publish_python_population_generation,
};
use crate::test_runner::python_coverage_index::{
    GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION,
};
use crate::test_runner::runners::detect_rslip_versions;
use crate::test_runner::runners::python_backer::PythonModule;
use crate::test_runner::{PlannedSelectors, SelectorRunOptions};
use kiss::rpytest_runner::TestStatus;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

fn planned(repo: &Path) -> PlannedSelectors {
    PlannedSelectors {
        repo_root: repo.to_path_buf(),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["t.py::test_a".into()],
            rust: Vec::new(),
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed { python: 0, rust: 0 },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        coverage_decision_engine_used: false,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::coverage_decision::SelectionBasis::Current,
            rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
        },
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    }
}

#[test]
fn hook_helpers_detect_current_generation_and_indexable_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selector = "t.py::test_a".to_string();
    let mut plan =
        population_plan_for_selectors(repo, std::slice::from_ref(&selector), &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.clone(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();

    let planned = planned(repo);
    let options = SelectorRunOptions {
        dry_run: false,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        plan_duration: Duration::ZERO,
        gate: kiss::GateConfig::default(),
    };
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    assert!(generation_already_current(
        &ctx,
        std::slice::from_ref(&selector)
    ));
    let module = PythonModule::new(repo, &[], &BTreeMap::new(), &[], &[], &[], &[]);
    assert!(is_indexable(&module, &repo.join("app.py"), repo));
    assert!(!is_indexable(&module, Path::new("/tmp/out.py"), repo));
    super::rebuild_python_index(&module, &ctx).unwrap();
    super::write_python_manifest(&module, &[selector], &ctx).unwrap();
    let force_options = SelectorRunOptions {
        force_rerun: true,
        ..options
    };
    let force_ctx = RunContext {
        planned: &planned,
        options: &force_options,
    };
    let _ = super::write_python_manifest(&module, &planned.sel.python, &force_ctx);
}

#[test]
fn rebuild_skips_when_selective_index_flag_set() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let mut planned = planned(repo);
    planned.skip_index_rebuild_after_selective.python = true;
    let options = SelectorRunOptions {
        dry_run: false,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        plan_duration: Duration::ZERO,
        gate: kiss::GateConfig::default(),
    };
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    let module = PythonModule::new(repo, &[], &BTreeMap::new(), &[], &[], &[], &[]);
    super::rebuild_python_index(&module, &ctx).unwrap();
}

#[test]
fn generation_already_current_is_false_without_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let planned = planned(repo);
    let options = SelectorRunOptions {
        dry_run: false,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        plan_duration: Duration::ZERO,
        gate: kiss::GateConfig::default(),
    };
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    assert!(!generation_already_current(&ctx, &planned.sel.python));
}
