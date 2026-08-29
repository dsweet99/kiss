use sha2::{Digest, Sha256};

use super::types::FullExecutionGeneration;

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn generation_id_for_payload(
    generation: &FullExecutionGeneration,
) -> Result<String, String> {
    let payload = generation.semantic_payload();
    let bytes = serde_json::to_vec(&payload)
        .map_err(|err| format!("error: kiss: serialize generation payload: {err}"))?;
    Ok(sha256_hex(&bytes))
}

pub(crate) fn manifest_digest(generation: &FullExecutionGeneration) -> Result<String, String> {
    let mut for_hash = generation.clone();
    for_hash.content_digest.clear();
    let bytes = serde_json::to_vec(&for_hash)
        .map_err(|err| format!("error: kiss: serialize generation manifest: {err}"))?;
    Ok(sha256_hex(&bytes))
}
