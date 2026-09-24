use std::path::Path;

use super::projection::slice_for;
use super::report::TargetReport;
use super::resolve::resolve_only;
use super::rows::rows_from_witnesses;
use super::types::TargetRequest;

pub(crate) fn recapture_report(
    repo_root: &Path,
    request: &TargetRequest,
    report: &TargetReport,
) -> Result<TargetReport, String> {
    let rows = rows_from_witnesses(repo_root, &report.scope)?;
    if rows != report.rows {
        return Err("concurrent mutation".into());
    }
    let resolved = resolve_only(repo_root, request)?;
    let stamp = slice_for(repo_root, request, &resolved);
    if stamp != report.stamp {
        return Err("concurrent mutation".into());
    }
    let mut recaptured = TargetReport::assembled_in(
        repo_root,
        request,
        report.scope.clone(),
        rows,
        stamp,
        report.exit_code,
        report.coverage_all,
    );
    recaptured.snapshot.extra.clone_from(&report.snapshot.extra);
    if recaptured.evidence != report.evidence
        || recaptured.snapshot.worktree != report.snapshot.worktree
        || recaptured.snapshot.gate_policy != report.snapshot.gate_policy
        || recaptured.snapshot.runner != report.snapshot.runner
        || recaptured.snapshot.python_witness != report.snapshot.python_witness
        || recaptured.snapshot.python_coverage != report.snapshot.python_coverage
        || recaptured.snapshot.rust_witness != report.snapshot.rust_witness
        || recaptured.snapshot.rust_coverage != report.snapshot.rust_coverage
        || recaptured.snapshot.resolved != report.snapshot.resolved
        || recaptured.snapshot.population != report.snapshot.population
        || recaptured.snapshot.configuration != report.snapshot.configuration
    {
        return Err("concurrent mutation".into());
    }
    Ok(recaptured)
}
