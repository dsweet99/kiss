use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::rust_coverage_index::{
    publish_rust_derived_state_with_filter, rust_population_manifest_is_current_for_args,
};

pub(super) fn repair_stale_population_on_all_mode_accept(
    request: &EnsureRequest,
    planned: &[String],
) -> bool {
    if request.mode != AcceptMode::All || planned.is_empty() {
        return false;
    }
    if rust_population_manifest_is_current_for_args(
        &request.repo_root,
        planned,
        &request.extras.rust,
    ) {
        return false;
    }
    publish_rust_derived_state_with_filter(
        &request.repo_root,
        Some(planned),
        &request.extras.rust,
        |_, _| true,
    )
    .is_ok()
}
