use super::*;
use crate::parsing::parse_files;
use std::path::Path;

#[test]
fn test_foil_cliff_weighted_pct_low() {
    let root = Path::new("/tmp/kiss_foil_shpaybxe");
    if !root.exists() {
        return;
    }
    let cliff = root.join("foil_py/cliffs/cliff_00.py");
    let test_py = root.join("tests/test_foil.py");
    let parsed: Vec<_> = parse_files(&[cliff.clone(), test_py])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let weighted = coverage::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&cliff).copied().unwrap_or(100);
    assert!(
        pct < 10,
        "foil cliff_00 weighted pct should be low, got {pct}% cov_map_keys={:?}",
        analysis.coverage_map.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_weighted_file_pct_discounts_high_branch_covered_def() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cliff = tmp.path().join("cliff.py");
    let mut body = String::from("def orchestrate(seed: int) -> int:\n    acc = seed\n");
    for i in 0..20 {
        body.push_str(&format!(
            "    if seed == {i}:\n        return acc + {i}\n    elif seed == {}:\n        acc += {}\n",
            i + 100,
            i
        ));
    }
    body.push_str("    return acc\n");
    std::fs::write(&cliff, &body).unwrap();

    let test_path = tmp.path().join("test_cliff.py");
    std::fs::write(
        &test_path,
        "from cliff import orchestrate\n\ndef test_cliff():\n    assert orchestrate(0) >= 0\n",
    )
    .unwrap();

    let paths = vec![cliff.clone(), test_path.clone()];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let weighted = coverage::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&cliff).copied().unwrap_or(100);
    assert!(
        pct < 20,
        "high-branch cliff should get low weighted pct, got {pct}% unreferenced={:?}",
        analysis.unreferenced.len()
    );
}
