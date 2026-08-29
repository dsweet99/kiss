use std::collections::BTreeMap;

use crate::rust_llvm_cov_runner::publish_derived::batch_derived::INDEX_SCHEMA_VERSION;
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::PopulationLoadMode;
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index_types::OnDiskIndexWithFiles;

#[test]
fn witness_population_load_mode_and_index_types() {
    let current = PopulationLoadMode::Current {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "context".to_string(),
        ordinary_source_digests: BTreeMap::new(),
    };
    let reusable = PopulationLoadMode::ReusablePrior {
        selection_context_fingerprint: "context".to_string(),
    };
    assert!(matches!(current, PopulationLoadMode::Current { .. }));
    assert!(matches!(reusable, PopulationLoadMode::ReusablePrior { .. }));

    let index = OnDiskIndexWithFiles {
        schema_version: INDEX_SCHEMA_VERSION.to_string(),
        source_root: "root".to_string(),
        generation_fingerprint: "generation".to_string(),
        entries_fingerprint: "entries".to_string(),
        files: BTreeMap::new(),
    };
    assert_eq!(index.source_root, "root");
}
