use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) const TOKEN_LEN: usize = 16;

pub(crate) fn random_token() -> [u8; TOKEN_LEN] {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    let mut token = [0u8; TOKEN_LEN];
    token[..4].copy_from_slice(&pid.to_le_bytes());
    token[4..8].copy_from_slice(&counter.to_le_bytes());
    token[8..16].copy_from_slice(&nanos.to_le_bytes());
    token
}

pub(crate) fn encode_token_hex(token: &[u8; TOKEN_LEN]) -> String {
    token.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn decode_token_hex(value: &str) -> Option<[u8; TOKEN_LEN]> {
    if value.len() != TOKEN_LEN * 2 {
        return None;
    }
    let mut token = [0u8; TOKEN_LEN];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let hex = std::str::from_utf8(chunk).ok()?;
        token[index] = u8::from_str_radix(hex, 16).ok()?;
    }
    Some(token)
}
