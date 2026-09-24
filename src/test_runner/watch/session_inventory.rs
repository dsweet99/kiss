use super::{LastReplies, RunTestCmdArgs, WatchSuiteReport};
use crate::test_runner::workspace_selector_cache::{
    load_cached_python_workspace_selectors, load_cached_rust_workspace_selectors,
};

pub(in super::super) fn reconcile_inventory(
    suite: &mut WatchSuiteReport,
    last: &LastReplies,
    args: &RunTestCmdArgs<'_>,
) -> bool {
    if !last.matches_args(args) {
        return true;
    }
    if let Some(selectors) =
        load_cached_python_workspace_selectors(&last.repo, args.ignore, args.python_extra)
    {
        suite.retain_language_selectors(kiss::Language::Python, &selectors);
    }
    let Some(selectors) = load_cached_rust_workspace_selectors(&last.repo, args.ignore) else {
        return true;
    };
    let report_ids = if selectors.is_empty() {
        Default::default()
    } else {
        match crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
            &last.repo,
            args.ignore,
        ) {
            Ok(ids) => ids,
            Err(err) => {
                eprintln!("error: kiss test --watch: {err}");
                return false;
            }
        }
    };
    suite.retain_rust_selectors(&selectors, &report_ids);
    true
}
