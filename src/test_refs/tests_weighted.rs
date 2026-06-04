use super::*;
use crate::parsing::parse_files;
use std::path::Path;

#[test]
fn test_foil_4ra8iatq_bind_only_big_fn_low_weighted() {
    let root = Path::new("/tmp/kiss_foil_4ra8iatq");
    if !root.exists() {
        return;
    }
    let mod_py = root.join("foil_py/mod.py");
    let test_py = root.join("tests/test_probe.py");
    let init_py = root.join("foil_py/__init__.py");
    let parsed: Vec<_> = parse_files(&[mod_py.clone(), init_py, test_py])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&mod_py).copied().unwrap_or(100);
    assert!(
        pct <= 5,
        "bind-only big_fn should yield low mod.py weighted pct, got {pct}% call_refs={:?}",
        analysis.call_references
    );
    assert!(!analysis.call_references.contains("big_fn"));
    assert!(analysis.call_references.contains("small_fn"));
}

#[test]
fn test_foil2u9_cliff_and_phantom_weighted() {
    let root = Path::new("/tmp/kiss_foil_2u9w9dd5");
    if !root.exists() {
        return;
    }
    let cliff = root.join("foil_py/cliffs/cliff_00.py");
    let phantom = root.join("foil_py/phantoms/phantom_00.py");
    let test_py = root.join("tests/test_foil.py");
    let parsed: Vec<_> = parse_files(&[cliff.clone(), phantom.clone(), test_py])
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let cliff_pct = weighted.get(&cliff).copied().unwrap_or(100);
    let phantom_pct = weighted.get(&phantom).copied().unwrap_or(100);
    assert!(
        (1..=10).contains(&cliff_pct),
        "cliff weighted pct should be low but non-zero, got {cliff_pct}%"
    );
    assert!(
        (1..=10).contains(&phantom_pct),
        "phantom bind-only weighted pct should be low, got {phantom_pct}%"
    );
    assert!(analysis.call_references.contains("orchestrate_00"));
    assert!(!analysis.call_references.contains("phantom_00"));
}

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
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&cliff).copied().unwrap_or(100);
    assert!(
        pct < 10,
        "foil cliff_00 weighted pct should be low, got {pct}% cov_map_keys={:?}",
        analysis.coverage_map.keys().collect::<Vec<_>>()
    );
}

#[test]
fn test_foil99_catalog_weighted_pct_with_full_catalog_set() {
    let root = Path::new("/tmp/kiss_foil_99rpziwm");
    if !root.exists() {
        return;
    }
    let mut paths: Vec<_> = std::fs::read_dir(root.join("foil_py/catalog"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "py"))
        .collect();
    paths.push(root.join("tests/test_foil.py"));
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let catalog = root.join("foil_py/catalog/catalog_00.py");
    let pct = weighted.get(&catalog).copied().unwrap_or(100);
    eprintln!(
        "full catalog set: pct={pct}% unref={}",
        analysis.unreferenced.len()
    );
    assert!(
        (5..=20).contains(&pct),
        "catalog weighted pct should be ~10% with full name collisions, got {pct}%"
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
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&cliff).copied().unwrap_or(100);
    assert!(
        pct < 20,
        "high-branch cliff should get low weighted pct, got {pct}% unreferenced={:?}",
        analysis.unreferenced.len()
    );
}
