use kiss::Language;

use super::target_request::TargetFocus;
use super::{PlannedSelectors, RunTestCmdArgs};

pub(crate) fn target_has_retry_bad(
    repo: &std::path::Path,
    request: &super::target_request::TargetRequest,
) -> bool {
    let Ok(resolved) = super::target_request::resolve_only(repo, request) else {
        return false;
    };
    let (projection, complete) =
        super::target_request::build_slice_projection(repo, request, &resolved);
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors.iter().cloned());
    let scope = super::target_request::ReportScope::from_membership(
        resolved.regions.clone(),
        selectors,
        complete,
    );
    let rows = super::target_request::available_rows(repo, &scope);
    if !super::target_request::plan_from_available_rows(&scope, &rows, true, false)
        .retry_bad
        .is_empty()
    {
        return true;
    }
    let Some(report) = super::target_request::load_current_report(repo) else {
        return false;
    };
    report.rows.iter().any(|row| {
        matches!(
            row.effective,
            super::target_request::EffectiveStatus::Fail
                | super::target_request::EffectiveStatus::Timeout
        ) && selector_under_request(repo, &row.selector, request)
    })
}

fn selector_under_request(
    repo: &std::path::Path,
    selector: &str,
    request: &super::target_request::TargetRequest,
) -> bool {
    let Some(raws) = super::target_request::operand_raws(&request.focus) else {
        return true;
    };
    let repo_s = repo.to_string_lossy();
    raws.iter().any(|raw| {
        let path = raw.split_once("::").map_or(raw.as_str(), |(path, _)| path);
        let rel = path
            .strip_prefix(repo_s.as_ref())
            .unwrap_or(path)
            .trim_start_matches('/');
        let symbol = raw.split_once("::").map(|(_, symbol)| symbol);
        match symbol {
            Some(symbol) => selector == format!("{rel}::{symbol}"),
            None => {
                selector == rel
                    || selector.starts_with(&format!("{rel}::"))
                    || selector.starts_with(&format!("{rel}/"))
            }
        }
    })
}

pub(crate) fn apply_force_bad(
    a: &RunTestCmdArgs<'_>,
    planned: &mut PlannedSelectors,
) -> Result<(), String> {
    if !a.force_bad {
        return Ok(());
    }
    let (python, rust) = typed_retry_by_lang(a, planned);    merge_lang_priors(a, planned, Language::Python, python);
    merge_lang_priors(a, planned, Language::Rust, rust);
    Ok(())
}

fn merge_lang_priors(
    a: &RunTestCmdArgs<'_>,
    planned: &mut PlannedSelectors,
    lang: Language,
    typed: Vec<String>,
) {
    let (sel, prior_sel) = match lang {
        Language::Python => (
            &mut planned.sel.python,
            &mut planned.prior_failure_selectors.python,
        ),
        Language::Rust => (
            &mut planned.sel.rust,
            &mut planned.prior_failure_selectors.rust,
        ),
    };
    merge_target_priors(
        &super::target_request::request_from_run_args(a).focus,
        sel,
        prior_sel,
        typed,
    );
}

fn typed_retry_by_lang(
    a: &RunTestCmdArgs<'_>,
    planned: &PlannedSelectors,
) -> (Vec<String>, Vec<String>) {
    let request = super::target_request::request_from_run_args(a);
    let Ok(resolved) = super::target_request::resolve_only(&planned.repo_root, &request) else {
        return (Vec::new(), Vec::new());
    };
    let (projection, complete) =
        super::target_request::build_slice_projection(&planned.repo_root, &request, &resolved);
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors.iter().cloned());
    selectors.extend(planned.sel.python.iter().cloned());
    selectors.extend(planned.sel.rust.iter().cloned());
    let scope =
        super::target_request::ReportScope::from_membership(resolved.regions, selectors, complete);
    let rows = super::target_request::available_rows(&planned.repo_root, &scope);
    let mut python = Vec::new();
    let mut rust = Vec::new();
    for selector in
        super::target_request::plan_from_available_rows(&scope, &rows, true, false).retry_bad
    {
        match rows
            .iter()
            .find(|row| row.selector == selector)
            .map(|row| row.language.as_str())
        {
            Some("python") => python.push(selector),
            Some("rust") => rust.push(selector),
            _ => {}
        }
    }
    (python, rust)
}

fn merge_target_priors(
    focus: &TargetFocus,
    planned_sel: &mut Vec<String>,
    prior_sel: &mut Vec<String>,
    bad: impl IntoIterator<Item = String>,
) {
    let extras: Vec<String> = bad
        .into_iter()
        .filter(|id| prior_belongs_to_target(focus, planned_sel, id))
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
    focus: &TargetFocus,
    planned_sel: &[String],
    selector: &str,
) -> bool {
    match focus {
        TargetFocus::Workspace => true,
        TargetFocus::Operands(operands) => {
            if operands
                .iter()
                .any(|operand| selector_in_target(selector, &operand.raw))
            {
                return true;
            }
            operands
                .iter()
                .all(|operand| !target_names_a_test(&operand.raw))
                && planned_sel.iter().any(|s| s == selector)
        }
        TargetFocus::Git(_) => planned_sel.iter().any(|s| s == selector),
    }
}

fn target_names_a_test(target: &str) -> bool {
    target.contains("::") || source_file_colon_symbol(target).is_some()
}

fn source_file_colon_symbol(target: &str) -> Option<(&str, &str)> {
    if target.contains("::") {
        return None;
    }
    let (path, name) = target.rsplit_once(':')?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    if path.ends_with(".py") || path.ends_with(".rs") {
        Some((path, name))
    } else {
        None
    }
}

pub(crate) fn selector_in_target(selector: &str, target: &str) -> bool {
    if selector == target {
        return true;
    }
    if let Some((path, name)) = source_file_colon_symbol(target) {
        return selector_in_target(selector, &format!("{path}::{name}"));
    }
    if target.contains("::") {
        return selector.starts_with(&format!("{target}::"))
            || selector.starts_with(&format!("{target}["))
            || selector.starts_with(&format!("{target}."));
    }
    let path = target.trim_end_matches('/');
    if selector.starts_with(&format!("{path}::")) || selector.starts_with(&format!("{path}/")) {
        return true;
    }
    let sel_path = selector.split("::").next().unwrap_or(selector);
    path == sel_path
        || path.ends_with(&format!("/{sel_path}"))
        || sel_path.ends_with(&format!("/{path}"))
}
