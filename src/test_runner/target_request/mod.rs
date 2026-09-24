mod adapt;
mod bind;
mod canon;
mod counters;
mod coverage;
mod digest;
mod ensure;
mod graph_store;
mod history;
mod manifest;
mod plan_store;
mod projection;
mod recapture;
mod render;
mod report;
mod report_store;
mod resolve;
mod resolved;
mod rows;
mod scope;
mod slice;
mod snapshot;
mod stamp;
mod types;

pub(crate) use adapt::{
    change_mode_from_focus, compat_matches, is_workspace_focus, is_workspace_run, operand_raws,
    request_from_invocation, request_from_run_args, to_compat_invocation,
};
#[cfg(test)]
pub(crate) use adapt::{
    focus_from_invocation, operands_request, request_from_focus, workspace_request,
};
#[cfg(test)]
pub(crate) use bind::load_ready_for_request;
pub(crate) use bind::{BindDecision, bind_and_prepare};
pub(crate) use counters::add_index;
pub(crate) use coverage::{coverage_exit_from_ready_request, coverage_paths_for_request};
pub(crate) use projection::build_slice_projection;
#[cfg(test)]
pub(crate) use projection::slice_for;
pub(crate) use render::official_report_text;
pub(crate) use report::{EffectiveStatus, TargetReport, configuration_generation, runner_identity};

#[cfg(test)]
pub(crate) use ensure::materialize_target_report;
pub(crate) use ensure::{
    EnsureOutcome, Ensured, assemble_target_report_query, ensure_target_report,
    ensure_target_report_query,
};
#[cfg(test)]
pub(crate) use report::SelectorRow;
#[cfg(test)]
pub(crate) use report_store::{publish_if_rows_hold, publish_report};
pub(crate) use resolve::resolve_only;
pub(crate) use rows::{available_rows, plan_from_available_rows};
pub(crate) use scope::ReportScope;
pub(crate) use snapshot::EnsurePolicy;
pub(crate) use types::{GitFocus, TargetFocus, TargetRequest};

#[allow(dead_code)]
pub(crate) fn remember_live_exit(args: &crate::test_runner::RunTestCmdArgs<'_>, exit_code: i32) {
    remember_named(args, exit_code, None);
}

pub(crate) fn remember_named(
    args: &crate::test_runner::RunTestCmdArgs<'_>,
    exit_code: i32,
    repo_root: Option<&std::path::Path>,
) {
    let repo_owned;
    let repo = match repo_root {
        Some(path) => path,
        None => {
            let Ok(cwd) = std::env::current_dir() else {
                return;
            };
            repo_owned = match crate::test_git::git_repo_root(&cwd) {
                Ok(path) => path,
                Err(_) => return,
            };
            &repo_owned
        }
    };
    let request = request_from_run_args(args);
    let Ok(resolved) = resolve::resolve_target(repo, &request) else {
        return;
    };
    let (projection, complete) = projection::build_slice_projection(repo, &request, &resolved);
    let stamp = slice::stamp_from_projection(&projection, complete);
    let mut selectors = projection.selectors();
    selectors.extend(resolved.direct_selectors);
    let scope =
        scope::ReportScope::from_membership(projection.coverage_regions(), selectors, complete);
    let Ok(rows) = rows::rows_from_witnesses(repo, &scope) else {
        return;
    };
    if rows.is_empty() || !stamp.complete {
        return;
    }
    let built = report::TargetReport::assembled_in(
        repo,
        &request,
        scope,
        rows.clone(),
        stamp,
        report::TargetReport::combine_exit(report::TargetReport::exit_from_rows(&rows), exit_code),
        args.coverage_all,
    );
    let _ = report_store::publish_if_rows_hold(repo, &request, &built);
    let _ = report_store::load_current_report(repo);
}

#[cfg(test)]
#[path = "request_test.rs"]
mod request_test;

#[cfg(test)]
#[path = "stamp_test.rs"]
mod stamp_test;

#[cfg(test)]
#[path = "resolve_test.rs"]
mod resolve_test;

#[cfg(test)]
#[path = "slice_test.rs"]
mod slice_test;

#[cfg(test)]
#[path = "store_test.rs"]
mod store_test;

#[cfg(test)]
#[path = "ensure_test.rs"]
mod ensure_test;
