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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch_shim::SHIM_LIST_SCHEMA;
    use std::path::PathBuf;

    fn shim(full_name: &str) -> BatchShimMetadata {
        BatchShimMetadata {
            schema_version: SHIM_LIST_SCHEMA.to_string(),
            id: full_name.to_string(),
            full_name: full_name.to_string(),
            profile_path: PathBuf::from("profile.profraw"),
            cwd: PathBuf::from("."),
            argv: vec!["test-bin".to_string()],
            exit_code: Some(0),
            spawn_error: None,
            shim_identity: None,
            delegated_identity: None,
            stdout: None,
            stderr: None,
            output_frame_count: None,
        }
    }

    #[test]
    fn resolves_exact_full_name_first() {
        let items = vec![shim("bin-a$case"), shim("bin-b$case")];
        let by_full_name: BTreeMap<_, _> = items
            .iter()
            .map(|item| (item.full_name.clone(), item))
            .collect();

        let resolved = resolve_shim_metadata(&by_full_name, &items, "bin-b$case").unwrap();

        assert_eq!(resolved.full_name, "bin-b$case");
    }

    #[test]
    fn resolves_unique_suffix_when_binary_id_changed() {
        let items = vec![shim("new-bin$case")];
        let by_full_name = BTreeMap::new();

        let resolved = resolve_shim_metadata(&by_full_name, &items, "old-bin$case").unwrap();

        assert_eq!(resolved.full_name, "new-bin$case");
    }

    #[test]
    fn reports_missing_metadata_without_suffix() {
        let items = vec![shim("bin$case")];
        let by_full_name = BTreeMap::new();

        let err = resolve_shim_metadata(&by_full_name, &items, "case").unwrap_err();

        assert!(format!("{err:?}").contains("missing target-runner metadata"));
    }

    #[test]
    fn reports_missing_and_ambiguous_suffix_matches() {
        let missing_items = vec![shim("bin$other")];
        let by_full_name = BTreeMap::new();
        let missing =
            resolve_shim_metadata(&by_full_name, &missing_items, "old-bin$case").unwrap_err();
        assert!(format!("{missing:?}").contains("missing target-runner metadata"));

        let ambiguous_items = vec![shim("bin-a$case"), shim("bin-b$case")];
        let ambiguous =
            resolve_shim_metadata(&by_full_name, &ambiguous_items, "old-bin$case").unwrap_err();
        assert!(format!("{ambiguous:?}").contains("ambiguous target-runner metadata"));
    }
}
