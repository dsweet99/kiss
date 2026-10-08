use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, GenerationIds};

pub(super) fn stored_witness(repo_root: &Path, extras: &[String]) -> Option<ExecutionWitness> {
    super::try_load_rust_execution_witness(repo_root, extras).ok()
}

pub(super) fn runner_identity_part(repo_root: &Path) -> Option<serde_json::Value> {
    let witness = super::try_load_rust_execution_witness(repo_root, &[]).ok()?;
    Some(serde_json::json!({
        "lang": "rust",
        "identity_digest": witness.identity_digest,
    }))
}

pub(super) fn generation_ids(repo_root: &Path) -> GenerationIds {
    GenerationIds {
        witness: super::try_load_rust_execution_witness(repo_root, &[])
            .ok()
            .map(|witness| witness.generation_id),
    }
}
