use std::collections::BTreeMap;

use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;

pub(super) enum CheckAggregateLoadMode {
    Current {
        input_fingerprint: String,
        generation_fingerprint: String,
        selection_context_fingerprint: String,
        ordinary_source_digests: BTreeMap<String, String>,
    },
    ReusablePrior {
        selection_context_fingerprint: String,
    },
}

pub(super) fn current_load_mode(identity: &RustCoverageBatchIdentity) -> CheckAggregateLoadMode {
    CheckAggregateLoadMode::Current {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        ordinary_source_digests: identity.ordinary_source_digests.clone(),
    }
}
