use super::*;
use std::path::PathBuf;

#[test]
fn phase_metrics_prints_zero_summary() {
    let phase = PhaseMetrics::default();

    assert_eq!(phase.summary.total, 0);
    print_phase_metrics("phase", &phase);
}

#[test]
fn rust_cache_unstored_sums_population_and_final_phases() {
    let mut metrics = empty_metrics();
    metrics.rust_population.summary.cache_unstored = 2;
    metrics.rust_final.summary.cache_unstored = 3;

    assert_eq!(rust_cache_unstored(&metrics), 5);
}

#[test]
fn metrics_print_helpers_accept_empty_metrics() {
    let metrics = empty_metrics();

    print_oracle_metrics();
    print_selection_metrics(&metrics);
    print_timing_metrics(&metrics);
    print_cache_metrics(&metrics);
    metrics.print();
}

#[test]
fn cache_shape_measures_the_kiss_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let records = tmp.path().join(".kiss").join("test").join("records");
    fs::create_dir_all(&records).unwrap();
    fs::write(records.join("a.json"), b"abc").unwrap();
    fs::write(records.join("b.json"), b"defg").unwrap();
    let mut metrics = empty_metrics();

    metrics.capture_cache_shape(tmp.path());

    assert_eq!(metrics.kiss_cache_residual_bytes, 7);
}

#[test]
fn cache_shape_is_zero_without_a_kiss_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let mut metrics = empty_metrics();
    metrics.kiss_cache_residual_bytes = 9;

    metrics.capture_cache_shape(tmp.path());

    assert_eq!(metrics.kiss_cache_residual_bytes, 0);
}

fn empty_metrics() -> LocalRubricMetrics {
    LocalRubricMetrics {
        selection_engine_used: true,
        rust_concurrency_budget: 1,
        ..Default::default()
    }
}

#[test]
fn local_rubric_metrics_carry_population_selection_basis() {
    use crate::test_runner::test_selection::SelectionBasis;
    use crate::test_runner::{PlannedSelectors, SelectorRunOptions};

    let planned = PlannedSelectors {
        repo_root: PathBuf::from("/repo"),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: vec!["tests::gets_value".to_string()],
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: vec![PathBuf::from("src/lib.rs")],
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 168,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        selection_engine_used: true,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::test_selection::SelectionBasis::Current,
            rust: SelectionBasis::Population,
        },
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    };
    let options = SelectorRunOptions {
        dry_run: true,
        jobs: 1,
        plan_duration: Duration::ZERO,
        force_rerun: false,
        metrics: false,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },

        gate: kiss::GateConfig::default(),
    };
    let metrics = LocalRubricMetrics::new(
        &planned,
        &options,
        0,
        false,
        0,
        planned.sel.rust.len(),
        planned.selection_basis.rust,
    );
    assert_eq!(metrics.selection_basis, SelectionBasis::Population);
    assert_eq!(metrics.rust_vcs_source_paths, 168);
    assert!(!metrics.rust_population_required);
}
