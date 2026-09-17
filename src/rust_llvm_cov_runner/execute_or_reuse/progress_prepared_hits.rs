use std::sync::{Mutex, OnceLock};

use crate::rust_llvm_cov_runner::RustLlvmCovOutcome;

type PreparedHitsHook = Box<dyn FnMut(&[RustLlvmCovOutcome]) + Send>;

fn prepared_hits_hook() -> &'static Mutex<Option<PreparedHitsHook>> {
    static PREPARED_HITS_HOOK: OnceLock<Mutex<Option<PreparedHitsHook>>> = OnceLock::new();
    PREPARED_HITS_HOOK.get_or_init(|| Mutex::new(None))
}

pub(super) fn clear_prepared_rust_cache_hits_hook() {
    *prepared_hits_hook()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

pub fn install_prepared_rust_cache_hits_hook(
    hook: impl FnMut(&[RustLlvmCovOutcome]) + Send + 'static,
) {
    *prepared_hits_hook()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(hook));
}

pub fn emit_prepared_rust_cache_hits(outcomes: &[RustLlvmCovOutcome]) {
    if outcomes.is_empty() {
        return;
    }
    if let Some(hook) = prepared_hits_hook()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_mut()
    {
        hook(outcomes);
    }
}
