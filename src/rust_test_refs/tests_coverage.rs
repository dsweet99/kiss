use super::compute_rs_weighted_file_pcts;
use crate::rust_parsing::parse_rust_file;
use crate::rust_test_refs::analyze_rust_test_refs;

#[test]
fn weighted_pct_low_for_new_only_worker() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    let mut body = String::from(
        "pub struct Worker;\nimpl Worker {\n    pub fn new() -> Self { Worker }\n",
    );
    for i in 0..10 {
        body.push_str(&format!(
            "    pub fn task_{i}(seed: u64) -> u64 {{\n        let mut acc = seed;\n        if seed == {i} {{ acc += 1; }} else if seed == {} {{ acc += 2; }}\n        acc\n    }}\n",
            i + 100
        ));
    }
    body.push_str("}\n#[cfg(test)]\nmod tests {\n    use super::Worker;\n    #[test]\n    fn worker_new_only() { let _ = Worker::new(); }\n}\n");
    std::fs::write(&src, &body).unwrap();

    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&src).copied().unwrap_or(100);
    assert!(
        pct < 10,
        "new-only worker should get low weighted pct, got {pct}%"
    );
}

#[test]
fn weighted_pct_zero_when_no_covered_defs_have_credit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("vault.rs");
    let mut body = String::new();
    for i in 0..5 {
        body.push_str(&format!("pub fn stage_{i}(v: u32) -> u32 {{ v + {i} }}\n"));
    }
    std::fs::write(&src, &body).unwrap();

    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&src).copied().unwrap_or(100);
    assert_eq!(pct, 0, "unreferenced-only file should be 0%");
}
