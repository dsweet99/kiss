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
fn weighted_pct_low_for_covered_orchestrate_only() {
    let root = std::path::Path::new("/tmp/kiss_foil_058jr492");
    if !root.exists() {
        return;
    }
    let mut paths: Vec<_> = std::fs::read_dir(root.join("src/cliffs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    paths.push(root.join("tests/foil_integration.rs"));
    paths.push(root.join("src/lib.rs"));
    paths.push(root.join("src/cliffs/mod.rs"));
    let parsed: Vec<_> = paths
        .iter()
        .filter_map(|p| parse_rust_file(p).ok())
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let cliff = root.join("src/cliffs/cliff_00.rs");
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&cliff).copied().unwrap_or(100);
    assert!(
        (1..=5).contains(&pct),
        "orchestrate-only cliff should yield low non-zero pct, got {pct}%"
    );
}

#[test]
fn weighted_pct_low_for_gfqe6dy_phantom_foil() {
    let root = std::path::Path::new("/tmp/kiss_foil_gfqe6dy_");
    if !root.exists() {
        return;
    }
    let mut paths: Vec<_> = std::fs::read_dir(root.join("src"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    paths.push(root.join("src/portal/mod.rs"));
    paths.push(root.join("tests/foil_integration.rs"));
    let parsed: Vec<_> = paths
        .iter()
        .filter_map(|p| parse_rust_file(p).ok())
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let phantom = root.join("src/phantom.rs");
    let pct = weighted.get(&phantom).copied().unwrap_or(100);
    assert!(
        pct <= 10,
        "gfqe6dy phantom should get low weighted pct, got {pct}%"
    );
    assert!(
        !analysis
            .call_references
            .iter()
            .any(|n| n.starts_with("slot_"))
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

#[test]
fn weighted_pct_low_for_gapd9k3n_worker() {
    let root = std::path::Path::new("/tmp/kiss_foil_gapd9k3n");
    let worker = root.join("src/impl_farm/worker_00.rs");
    if !worker.exists() {
        return;
    }
    let mut paths: Vec<_> = std::fs::read_dir(root.join("src"))
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension() == Some(std::ffi::OsStr::new("rs"))).then_some(p)
        })
        .collect();
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    walk(&root.join("src/impl_farm"), &mut paths);
    paths.push(root.join("tests/foil_integration.rs"));
    let parsed: Vec<_> = paths
        .iter()
        .filter_map(|p| parse_rust_file(p).ok())
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&worker).copied().unwrap_or(100);
    assert!(
        pct < 2,
        "gapd9k3n worker with full src set and graph should be ~1%, got {pct}%"
    );
}
