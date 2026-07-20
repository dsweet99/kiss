use super::test_binaries_from_shim_metadata;
use crate::batch_shim::BatchShimMetadata;

fn shim_rows_for_bin(
    tmp: &std::path::Path,
    bin: &std::path::Path,
    n: usize,
) -> Vec<BatchShimMetadata> {
    (0..n)
        .map(|i| BatchShimMetadata {
            schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
            id: format!("t{i}"),
            full_name: format!("pkg::bin$t{i}"),
            profile_path: tmp.join(format!("t{i}.profraw")),
            cwd: tmp.to_path_buf(),
            argv: vec![bin.to_string_lossy().to_string()],
            exit_code: Some(0),
            spawn_error: None,
            shim_identity: None,
            delegated_identity: None,
            stdout: None,
            stderr: None,
            output_frame_count: None,
        })
        .collect()
}

#[test]
#[cfg(unix)]
fn test_binaries_from_shim_metadata_avoids_per_row_digest_amplification() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("fat-bin");
    // Large enough that N full re-reads would dominate /proc/self/io rchar.
    let payload = vec![0x5au8; 256 * 1024];
    std::fs::write(&bin, &payload).unwrap();
    let items = shim_rows_for_bin(tmp.path(), &bin, 80);
    let before = proc_rchar().expect("rchar");
    let binaries = test_binaries_from_shim_metadata(&items).unwrap();
    let after = proc_rchar().expect("rchar");
    assert_eq!(binaries.len(), 1);
    let delta = after.saturating_sub(before);
    // Bug: 80 × 256KiB ≈ 20MiB. Fix: one digest ≈ 256KiB (+ small overhead).
    assert!(
        delta < 2 * 1024 * 1024,
        "expected single digest of fat binary, rchar delta={delta}"
    );
}

/// Metamorphic: duplicate rows must not change the identity set vs a single row.
#[test]
fn test_binaries_from_shim_metadata_duplicates_are_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::write(&bin, b"abc").unwrap();
    let once = test_binaries_from_shim_metadata(&shim_rows_for_bin(tmp.path(), &bin, 1)).unwrap();
    let many = test_binaries_from_shim_metadata(&shim_rows_for_bin(tmp.path(), &bin, 17)).unwrap();
    assert_eq!(once, many);
}

/// Fuzz: for any seed, N≥1 duplicate rows for one binary yield exactly one identity.
#[test]
fn test_binaries_from_shim_metadata_fuzz_duplicate_count() {
    let seed = std::env::var("KISS_DIGEST_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
        });
    eprintln!("KISS_DIGEST_FUZZ_SEED={seed}");
    let mut state = seed;
    let next = |s: &mut u64| -> u64 {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
        *s
    };
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::write(&bin, b"fuzz-payload").unwrap();
    for _ in 0..32 {
        let n = (next(&mut state) % 64) + 1;
        let binaries =
            test_binaries_from_shim_metadata(&shim_rows_for_bin(tmp.path(), &bin, n as usize))
                .unwrap();
        assert_eq!(binaries.len(), 1, "seed={seed} n={n}");
    }
}

#[cfg(unix)]
fn proc_rchar() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/io").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("rchar: ").and_then(|v| v.parse().ok()))
}
