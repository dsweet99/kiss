use super::*;

use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn derived_state_publication_waits_for_derived_lock() {
    // Assert serialization only: blocked while held, then acquires after drop.
    // Do not bound post-release latency — under parallel `kiss test` load that
    // bound flaked (elapsed included the probe window + scheduler delay).
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    let guard = kiss::rslip::lock_rslip_state(&cache_root).unwrap();
    let cache_root_for_thread = cache_root.clone();
    let (tx, rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let _second = kiss::rslip::lock_rslip_state(&cache_root_for_thread).unwrap();
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "second lock must not complete while the first guard is held"
    );
    drop(guard);
    rx.recv_timeout(Duration::from_secs(2))
        .expect("waiter should acquire the derived lock after the first guard drops");
    waiter.join().unwrap();
}
