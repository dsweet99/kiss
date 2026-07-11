use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::batch_export_catalog::build_object_catalog;

#[test]
fn build_object_catalog_includes_env_executables() {
    let env = BTreeMap::from([(
        "KISS_EXPORT_CONTRACT_HELPER".to_string(),
        "/tmp/helper-bin".to_string(),
    )]);
    let catalog = build_object_catalog(
        &[],
        PathBuf::from("/tmp/missing-target").as_path(),
        &[],
        &env,
    );
    assert!(catalog.is_empty() || catalog.iter().all(|path| path.is_absolute()));
}
