use super::python_coverage_index::{
    rebuild_python_coverage_index, write_python_population_manifest_for_args,
};
use super::rust_coverage_index::{
    rebuild_rust_coverage_index, select_rust_source_selectors_from_index,
    select_rust_source_selectors_hybrid, write_rust_population_manifest_for_args,
};
use super::{PlannedSelectors, SelectorRunOptions, runners};
use std::time::Instant;

#[path = "run_logic/metrics.rs"]
mod metrics;
use metrics::LocalRubricMetrics;
use runners::SelectorExecutionSummary;

pub(crate) fn run_selectors(
    planned: &PlannedSelectors,
    options: SelectorRunOptions<'_>,
) -> Result<i32, String> {
    let total_started = Instant::now();
    if options.jobs == 0 {
        return Err("error: kiss test: jobs must be greater than zero".to_string());
    }
    if !planned_has_work(planned) {
        return Ok(finish_no_work(planned, &options, total_started));
    }
    let mut rs_sel = planned.rs_sel.clone();
    let needs_population = needs_rust_population(planned, &options);
    let population_selectors = rust_population_selectors_for_run(planned, needs_population)?;
    if options.dry_run {
        return Ok(finish_dry_run(
            planned,
            &options,
            total_started,
            needs_population,
            &population_selectors,
            &rs_sel,
        ));
    }
    run_selected_phases(
        planned,
        &options,
        total_started,
        needs_population,
        &population_selectors,
        &mut rs_sel,
    )
}

fn finish_no_work(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
) -> i32 {
    println!("{}", runners::NO_COVERING_TESTS_MSG);
    if options.metrics {
        let mut metrics = LocalRubricMetrics::new(planned, options, false, 0, planned.rs_sel.len());
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    0
}

fn rust_population_selectors_for_run(
    planned: &PlannedSelectors,
    needs_population: bool,
) -> Result<Vec<String>, String> {
    if needs_population {
        runners::enumerate_workspace_rust_selectors(&planned.repo_root, &planned.ignore)
    } else {
        Ok(Vec::new())
    }
}

fn finish_dry_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
    needs_population: bool,
    population_selectors: &[String],
    rs_sel: &[String],
) -> i32 {
    print_dry_run(
        planned,
        options,
        needs_population,
        population_selectors,
        rs_sel,
    );
    if options.metrics {
        let mut metrics = LocalRubricMetrics::new(
            planned,
            options,
            needs_population,
            population_selectors.len(),
            rs_sel.len(),
        );
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    0
}

fn run_selected_phases(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
    needs_population: bool,
    population_selectors: &[String],
    rs_sel: &mut Vec<String>,
) -> Result<i32, String> {
    let mut metrics = LocalRubricMetrics::new(
        planned,
        options,
        needs_population,
        population_selectors.len(),
        rs_sel.len(),
    );
    let python_started = Instant::now();
    metrics.python.summary = run_python_phase(planned, options)?;
    metrics.python.duration = python_started.elapsed();
    let mut code = metrics.python.summary.exit_code;
    if planned.python_population_required && code == 0 {
        rebuild_python_coverage_index(&planned.repo_root)?;
        write_python_population_manifest_for_args(
            &planned.repo_root,
            &planned.python_population_selectors,
            options.extra,
        )?;
    }
    if needs_population {
        code = run_population_phase(planned, options, population_selectors, &mut metrics, code)?;
        if code != 0 {
            return Ok(finish_run_metrics(
                metrics,
                code,
                total_started,
                planned,
                options,
            ));
        }
        write_rust_population_manifest_for_args(
            &planned.repo_root,
            population_selectors,
            options.extra,
        )?;
        extend_rust_source_selectors(planned, rs_sel);
        metrics.rust_final_selectors = rs_sel.len();
    }
    code = run_final_rust_phase(planned, options, rs_sel, &mut metrics, code)?;
    if needs_population && code == 0 {
        write_rust_population_manifest_for_args(
            &planned.repo_root,
            population_selectors,
            options.extra,
        )?;
    }
    Ok(finish_run_metrics(
        metrics,
        code,
        total_started,
        planned,
        options,
    ))
}

fn run_population_phase(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    population_selectors: &[String],
    metrics: &mut LocalRubricMetrics,
    code: i32,
) -> Result<i32, String> {
    let population_started = Instant::now();
    metrics.rust_population.summary = run_rust_population(planned, options, population_selectors)?;
    metrics.rust_population.duration = population_started.elapsed();
    let index_started = Instant::now();
    rebuild_rust_coverage_index(&planned.repo_root)?;
    metrics.rust_index_rebuild_duration += index_started.elapsed();
    Ok(runners::merge_exit_codes(
        code,
        metrics.rust_population.summary.exit_code,
    ))
}

fn run_final_rust_phase(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    rs_sel: &[String],
    metrics: &mut LocalRubricMetrics,
    code: i32,
) -> Result<i32, String> {
    let final_started = Instant::now();
    metrics.rust_final.summary = run_final_rust_selectors(planned, options, rs_sel)?;
    metrics.rust_final.duration = final_started.elapsed();
    if metrics.rust_final.summary.total > 0 {
        let index_started = Instant::now();
        rebuild_rust_coverage_index(&planned.repo_root)?;
        metrics.rust_index_rebuild_duration += index_started.elapsed();
    }
    Ok(runners::merge_exit_codes(
        code,
        metrics.rust_final.summary.exit_code,
    ))
}

fn planned_has_work(planned: &PlannedSelectors) -> bool {
    planned.python_population_required
        || !planned.py_sel.is_empty()
        || !planned.rs_sel.is_empty()
        || !planned.rust_source_population_paths.is_empty()
}

fn needs_rust_population(planned: &PlannedSelectors, options: &SelectorRunOptions<'_>) -> bool {
    !planned.rust_source_population_paths.is_empty()
        || (options.force_rerun && !planned.rust_source_paths.is_empty())
}

fn print_dry_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    needs_population: bool,
    population_selectors: &[String],
    rs_sel: &[String],
) {
    let py_argv = runners::build_pytest_argv(&planned.py_sel, options.extra);
    if planned.python_population_required {
        println!("PYTHON COVERAGE POPULATION");
        if !planned.python_population_selectors.is_empty() {
            let argv =
                runners::build_pytest_argv(&planned.python_population_selectors, options.extra);
            println!("{}", runners::shell_quote_line(&argv));
        }
    } else if !planned.py_sel.is_empty() {
        println!("{}", runners::shell_quote_line(&py_argv));
    }
    if needs_population {
        println!("RUST COVERAGE POPULATION");
    }
    for selector in population_selectors.iter().chain(rs_sel.iter()) {
        let argv = runners::build_cargo_llvm_cov_dry_run_argv(selector, options.extra);
        println!("{}", runners::shell_quote_line(&argv));
    }
}

fn run_python_selectors(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> Result<SelectorExecutionSummary, String> {
    if planned.py_sel.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    let force_rerun = options.force_rerun || !planned.python_prior_failure_selectors.is_empty();
    runners::run_rslip_selectors(
        &planned.repo_root,
        &planned.py_sel,
        options.extra,
        force_rerun,
        options.jobs,
    )
}

fn run_python_phase(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> Result<SelectorExecutionSummary, String> {
    if planned.python_population_required {
        run_python_population(planned, options)
    } else {
        run_python_selectors(planned, options)
    }
}

fn run_python_population(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> Result<SelectorExecutionSummary, String> {
    if !planned.python_population_required {
        return Ok(SelectorExecutionSummary::default());
    }
    let force_rerun = options.force_rerun || !planned.python_prior_failure_selectors.is_empty();
    runners::run_rslip_selectors(
        &planned.repo_root,
        &planned.python_population_selectors,
        options.extra,
        force_rerun,
        options.jobs,
    )
}

fn run_rust_population(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    population_selectors: &[String],
) -> Result<SelectorExecutionSummary, String> {
    let force_rerun = options.force_rerun || !planned.rust_prior_failure_selectors.is_empty();
    runners::run_rust_llvm_cov_selectors(
        &planned.repo_root,
        population_selectors,
        options.extra,
        force_rerun,
        options.jobs,
    )
}

fn extend_rust_source_selectors(planned: &PlannedSelectors, rs_sel: &mut Vec<String>) {
    let dynamic_selectors = if planned.rust_changed_lines.is_empty() {
        select_rust_source_selectors_from_index(&planned.repo_root, &planned.rust_source_paths)
    } else {
        select_rust_source_selectors_hybrid(
            &planned.repo_root,
            &planned.rust_source_paths,
            &planned.rust_changed_lines,
        )
        .or_else(|| {
            select_rust_source_selectors_from_index(&planned.repo_root, &planned.rust_source_paths)
        })
    }
    .unwrap_or_default();
    rs_sel.extend(dynamic_selectors);
    rs_sel.sort();
    rs_sel.dedup();
}

fn run_final_rust_selectors(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    rs_sel: &[String],
) -> Result<SelectorExecutionSummary, String> {
    if rs_sel.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    let force_rerun = options.force_rerun || !planned.rust_prior_failure_selectors.is_empty();
    runners::run_rust_llvm_cov_selectors(
        &planned.repo_root,
        rs_sel,
        options.extra,
        force_rerun,
        options.jobs,
    )
}

fn finish_run_metrics(
    mut metrics: LocalRubricMetrics,
    code: i32,
    total_started: Instant,
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> i32 {
    metrics.exit_code = code;
    metrics.total_duration = total_started.elapsed();
    metrics.capture_cache_shape(&planned.repo_root);
    if options.metrics {
        metrics.print();
    }
    code
}
