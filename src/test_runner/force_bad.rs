use kiss::Language;

use crate::bin_cli::args::TestInvocation;

use super::runners;
use super::{PlannedSelectors, RunTestCmdArgs};

pub(crate) fn apply_force_bad(
    a: &RunTestCmdArgs<'_>,
    planned: &mut PlannedSelectors,
) -> Result<(), String> {
    if !a.force_bad {
        return Ok(());
    }
    let py_bad = runners::current_prior_failures(
        &planned.repo_root,
        Language::Python,
        a.python_extra,
        &planned.ignore,
    )?;
    let rs_bad = runners::current_prior_failures(
        &planned.repo_root,
        Language::Rust,
        a.extra,
        &planned.ignore,
    )?;
    merge_target_priors(
        &a.invocation,
        &mut planned.sel.python,
        &mut planned.prior_failure_selectors.python,
        py_bad.into_iter().map(|s| s.id),
    );
    merge_target_priors(
        &a.invocation,
        &mut planned.sel.rust,
        &mut planned.prior_failure_selectors.rust,
        rs_bad.into_iter().map(|s| s.id),
    );
    Ok(())
}

fn merge_target_priors(
    invocation: &TestInvocation,
    planned_sel: &mut Vec<String>,
    prior_sel: &mut Vec<String>,
    bad: impl IntoIterator<Item = String>,
) {
    let extras: Vec<String> = bad
        .into_iter()
        .filter(|id| prior_belongs_to_target(invocation, planned_sel, id))
        .collect();
    prior_sel.extend(extras.iter().cloned());
    prior_sel.sort();
    prior_sel.dedup();
    for id in extras {
        if !planned_sel.iter().any(|s| s == &id) {
            planned_sel.push(id);
        }
    }
}

pub(crate) fn prior_belongs_to_target(
    invocation: &TestInvocation,
    planned_sel: &[String],
    selector: &str,
) -> bool {
    match invocation {
        TestInvocation::All => true,
        TestInvocation::Targets(targets) => {
            planned_sel.iter().any(|s| s == selector)
                || targets.iter().any(|t| selector_in_target(selector, t))
        }
        TestInvocation::Commit | TestInvocation::Base | TestInvocation::Main => {
            planned_sel.iter().any(|s| s == selector)
        }
    }
}

pub(crate) fn selector_in_target(selector: &str, target: &str) -> bool {
    if selector == target {
        return true;
    }
    if target.contains("::") {
        return selector.starts_with(&format!("{target}::"))
            || selector.starts_with(&format!("{target}["))
            || selector.starts_with(&format!("{target}."));
    }
    let path = target.trim_end_matches('/');
    selector.starts_with(&format!("{path}::")) || selector.starts_with(&format!("{path}/"))
}
