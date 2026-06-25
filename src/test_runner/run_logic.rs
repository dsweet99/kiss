use super::rust_coverage_index::{
    rebuild_rust_coverage_index, select_rust_source_selectors_from_index,
};
use super::{PlannedSelectors, SelectorRunOptions, runners};

pub(crate) fn run_selectors(
    planned: &PlannedSelectors,
    options: SelectorRunOptions<'_>,
) -> Result<i32, String> {
    if options.jobs == 0 {
        return Err("error: kiss test: jobs must be greater than zero".to_string());
    }
    if !planned_has_work(planned) {
        println!("{}", runners::NO_COVERING_TESTS_MSG);
        return Ok(0);
    }
    let mut rs_sel = planned.rs_sel.clone();
    let needs_population = needs_rust_population(planned, &options);
    let population_selectors = if needs_population {
        runners::enumerate_workspace_rust_selectors(&planned.repo_root, &planned.ignore)?
    } else {
        Vec::new()
    };
    if options.dry_run {
        print_dry_run(
            planned,
            &options,
            needs_population,
            &population_selectors,
            &rs_sel,
        );
        return Ok(0);
    }
    let mut code = run_python_selectors(planned, &options)?;
    if needs_population {
        code = run_rust_population(planned, &options, &population_selectors, code)?;
        if code != 0 {
            return Ok(code);
        }
        extend_rust_source_selectors(planned, &mut rs_sel);
    }
    run_final_rust_selectors(planned, &options, &rs_sel, code)
}

fn planned_has_work(planned: &PlannedSelectors) -> bool {
    !planned.py_sel.is_empty()
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
    if !planned.py_sel.is_empty() {
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
) -> Result<i32, String> {
    if planned.py_sel.is_empty() {
        return Ok(0);
    }
    runners::run_rslip_selectors(
        &planned.repo_root,
        &planned.py_sel,
        options.extra,
        options.force_rerun,
        options.jobs,
    )
}

fn run_rust_population(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    population_selectors: &[String],
    code: i32,
) -> Result<i32, String> {
    let rust_code = runners::run_rust_llvm_cov_selectors(
        &planned.repo_root,
        population_selectors,
        options.extra,
        options.force_rerun,
        options.jobs,
    )?;
    rebuild_rust_coverage_index(&planned.repo_root)?;
    Ok(runners::merge_exit_codes(code, rust_code))
}

fn extend_rust_source_selectors(planned: &PlannedSelectors, rs_sel: &mut Vec<String>) {
    let dynamic_selectors =
        select_rust_source_selectors_from_index(&planned.repo_root, &planned.rust_source_paths)
            .unwrap_or_default();
    rs_sel.extend(dynamic_selectors);
    rs_sel.sort();
    rs_sel.dedup();
}

fn run_final_rust_selectors(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    rs_sel: &[String],
    code: i32,
) -> Result<i32, String> {
    if rs_sel.is_empty() {
        return Ok(code);
    }
    let rust_code = runners::run_rust_llvm_cov_selectors(
        &planned.repo_root,
        rs_sel,
        options.extra,
        options.force_rerun,
        options.jobs,
    )?;
    rebuild_rust_coverage_index(&planned.repo_root)?;
    Ok(runners::merge_exit_codes(code, rust_code))
}
