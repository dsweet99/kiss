use std::path::Path;
use std::time::Duration;

pub fn try_load_sealed_population_durations(
    cache_root: &Path,
    source_root: &Path,
) -> Option<Vec<(String, Duration)>> {
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_identity_seal::try_source_matched_seal_identity(
            cache_root,
            source_root,
        )?;
    if !crate::rust_llvm_cov_runner::current_population_manifest_matches_identity(
        cache_root, &identity,
    )
    .unwrap_or(false)
    {
        return None;
    }
    super::batch_population_durations::try_load_durations_from_manifest_sidecar(
        cache_root, &identity, None,
    )
}
