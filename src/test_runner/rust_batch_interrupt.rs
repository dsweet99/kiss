//! Process-wide flag set when a Rust coverage batch is interrupted (Ctrl-C).

use std::sync::atomic::{AtomicBool, Ordering};

static RUST_BATCH_INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn consume_rust_batch_interrupted() -> bool {
    RUST_BATCH_INTERRUPTED.swap(false, Ordering::SeqCst)
}

pub(crate) fn note_rust_batch_interrupted() {
    RUST_BATCH_INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_then_consume_round_trips() {
        let _ = consume_rust_batch_interrupted();
        assert!(!consume_rust_batch_interrupted());
        note_rust_batch_interrupted();
        assert!(consume_rust_batch_interrupted());
        assert!(!consume_rust_batch_interrupted());
    }
}
