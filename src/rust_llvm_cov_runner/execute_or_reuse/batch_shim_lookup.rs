use std::collections::BTreeMap;

use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim::BatchShimMetadata;

pub(crate) fn resolve_shim_metadata<'a>(
    metadata_by_full_name: &BTreeMap<String, &'a BatchShimMetadata>,
    shim_metadata: &'a [BatchShimMetadata],
    test_full_name: &str,
) -> Result<&'a BatchShimMetadata, RustLlvmCovError> {
    if let Some(item) = metadata_by_full_name.get(test_full_name) {
        return Ok(*item);
    }
    let Some((binary_id, test_name)) = test_full_name.rsplit_once('$') else {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "missing target-runner metadata for test instance `{test_full_name}`"
        )));
    };
    let suffix_matches: Vec<_> = shim_metadata
        .iter()
        .filter(|item| {
            item.full_name
                .rsplit_once('$')
                .is_some_and(|(_, name)| name == test_name)
        })
        .collect();
    match suffix_matches.len() {
        0 => Err(RustLlvmCovError::InvalidRequest(format!(
            "missing target-runner metadata for test instance `{test_full_name}`"
        ))),
        1 => Ok(suffix_matches[0]),
        _ => {
            let compatible: Vec<_> = suffix_matches
                .into_iter()
                .filter(|item| {
                    item.full_name
                        .rsplit_once('$')
                        .is_some_and(|(shim_bin, _)| binary_ids_compatible(binary_id, shim_bin))
                })
                .collect();
            match compatible.len() {
                1 => Ok(compatible[0]),
                0 => Err(RustLlvmCovError::InvalidRequest(format!(
                    "missing target-runner metadata for test instance `{test_full_name}`"
                ))),
                _ => Err(RustLlvmCovError::InvalidRequest(format!(
                    "ambiguous target-runner metadata for test instance `{test_full_name}`"
                ))),
            }
        }
    }
}

fn binary_ids_compatible(expected: &str, shim_binary: &str) -> bool {
    if expected == shim_binary {
        return true;
    }
    let leaf = expected.rsplit("::").next().unwrap_or(expected);
    if shim_binary == leaf {
        return true;
    }
    if shim_binary.rsplit("::").next() == Some(leaf) {
        return true;
    }

    shim_binary
        .strip_prefix(leaf)
        .is_some_and(|rest| rest.starts_with('-') && rest.len() > 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim::SHIM_LIST_SCHEMA;
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

        let ambiguous_items = vec![shim("aclick_usage-aaa$case"), shim("aclick_usage-bbb$case")];
        let ambiguous = resolve_shim_metadata(
            &by_full_name,
            &ambiguous_items,
            "sameq_style::aclick_usage$case",
        )
        .unwrap_err();
        assert!(format!("{ambiguous:?}").contains("ambiguous target-runner metadata"));

        let unrelated = vec![shim("bin-a$case"), shim("bin-b$case")];
        let missing_compat =
            resolve_shim_metadata(&by_full_name, &unrelated, "old-bin$case").unwrap_err();
        assert!(format!("{missing_compat:?}").contains("missing target-runner metadata"));
    }

    #[test]
    fn disambiguates_shared_test_names_via_hashed_binary_stem() {
        let items = vec![
            shim("aclick_usage-abc123$kiss_bare_rule_api"),
            shim("comment_tags-def456$kiss_bare_rule_api"),
        ];
        let by_full_name = BTreeMap::new();
        let resolved = resolve_shim_metadata(
            &by_full_name,
            &items,
            "sameq_style::aclick_usage$kiss_bare_rule_api",
        )
        .unwrap();
        assert_eq!(resolved.full_name, "aclick_usage-abc123$kiss_bare_rule_api");
    }
}
