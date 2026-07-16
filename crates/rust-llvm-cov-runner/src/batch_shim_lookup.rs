use std::collections::BTreeMap;

use crate::RustLlvmCovError;
use crate::batch_shim::BatchShimMetadata;

pub(crate) fn resolve_shim_metadata<'a>(
    metadata_by_full_name: &BTreeMap<String, &'a BatchShimMetadata>,
    shim_metadata: &'a [BatchShimMetadata],
    test_full_name: &str,
) -> Result<&'a BatchShimMetadata, RustLlvmCovError> {
    if let Some(item) = metadata_by_full_name.get(test_full_name) {
        return Ok(*item);
    }
    let Some((_, test_name)) = test_full_name.rsplit_once('$') else {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "missing target-runner metadata for test instance `{test_full_name}`"
        )));
    };
    let matches: Vec<_> = shim_metadata
        .iter()
        .filter(|item| {
            item.full_name
                .rsplit_once('$')
                .is_some_and(|(_, name)| name == test_name)
        })
        .collect();
    match matches.len() {
        0 => Err(RustLlvmCovError::InvalidRequest(format!(
            "missing target-runner metadata for test instance `{test_full_name}`"
        ))),
        1 => Ok(matches[0]),
        _ => Err(RustLlvmCovError::InvalidRequest(format!(
            "ambiguous target-runner metadata for test instance `{test_full_name}`"
        ))),
    }
}
