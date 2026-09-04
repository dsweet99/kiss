use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::rust_coverage_index::{
    publish_rust_derived_state_with_filter, rust_population_manifest_is_current_for_args,
};

pub(super) fn repair_stale_population_on_all_mode_accept(
    request: &EnsureRequest,
    planned: &[String],
) -> Result<bool, String> {
    if planned.is_empty() {
        return Ok(false);
    }
    if rust_population_manifest_is_current_for_args(
        &request.repo_root,
        planned,
        &request.extras.rust,
    ) {
        return Ok(false);
    }
    if request.mode != AcceptMode::All
        && crate::test_runner::rust_coverage_index::rust_selective_rebuild_publication_selectors(
            &request.repo_root,
            planned,
            &request.extras.rust,
        )
        .is_none()
    {
        return Ok(false);
    }
    publish_rust_derived_state_with_filter(
        &request.repo_root,
        Some(planned),
        &request.extras.rust,
        |_, _| true,
    )?;
    Ok(true)
}
