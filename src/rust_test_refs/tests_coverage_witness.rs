use super::compute_rs_weighted_file_pcts;
use crate::rust_parsing::parse_rust_file;
use crate::rust_test_refs::analyze_rust_test_refs;

#[test]
fn live_call_witnesses_unreached_as_referenced() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    std::fs::write(
        &src,
        r#"pub fn covered() -> i32 { 1 }

pub fn unreached() -> i32 {
    let mut total = 0;
    for step in 0..60 {
        total += step;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::{covered, unreached};

    #[test]
    fn test_covered() {
        assert_eq!(covered(), 1);
    }

    #[test]
    fn calls_unreached() {
        assert!(unreached() >= 0);
    }
}
"#,
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let unref_names: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .filter(|d| d.file == src)
        .map(|d| d.name.as_str())
        .collect();
    assert!(!unref_names.contains("unreached"));
    assert!(
        analysis.test_references.contains("unreached")
            || analysis.call_references.contains("unreached"),
        "live call should mark unreached as referenced"
    );
}

#[test]
fn weighted_pct_monotone_in_call_depth() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("chain.rs");
    let prod = r#"pub fn leaf(n: i32) -> i32 { n + 1 }
pub fn mid(n: i32) -> i32 { leaf(n) + 1 }
pub fn root(n: i32) -> i32 { mid(n) + 1 }
"#;
    let shallow_test = format!(
        "{prod}#[cfg(test)]\nmod tests {{\n    use super::root;\n    #[test]\n    fn calls_root() {{ assert!(root(1) > 0); }}\n}}\n"
    );
    let deep_test = format!(
        "{prod}#[cfg(test)]\nmod tests {{\n    use super::{{root, mid, leaf}};\n    #[test]\n    fn calls_all() {{ assert!(root(1) + mid(1) + leaf(1) > 0); }}\n}}\n"
    );
    std::fs::write(&src, &shallow_test).unwrap();
    let shallow_parsed = parse_rust_file(&src).unwrap();
    let shallow_refs: Vec<_> = [&shallow_parsed].into_iter().collect();
    let shallow_analysis = analyze_rust_test_refs(&shallow_refs, None);
    let shallow_weighted = compute_rs_weighted_file_pcts(&shallow_analysis, &shallow_refs);
    let shallow_pct = shallow_weighted.get(&src).copied().unwrap_or(0);

    std::fs::write(&src, &deep_test).unwrap();
    let deep_parsed = parse_rust_file(&src).unwrap();
    let deep_refs: Vec<_> = [&deep_parsed].into_iter().collect();
    let deep_analysis = analyze_rust_test_refs(&deep_refs, None);
    let deep_weighted = compute_rs_weighted_file_pcts(&deep_analysis, &deep_refs);
    let deep_pct = deep_weighted.get(&src).copied().unwrap_or(0);

    assert!(
        deep_pct >= shallow_pct,
        "deeper call witnesses should not decrease weighted pct, shallow={shallow_pct}% deep={deep_pct}%"
    );
}

fn bypass_witness_status(src: &str) -> (bool, bool, bool) {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("lib.rs");
    std::fs::write(&path, src).unwrap();
    let parsed = parse_rust_file(&path).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let covered = !analysis
        .unreferenced
        .iter()
        .any(|d| d.name == "never_called");
    (
        covered,
        analysis.test_references.contains("never_called"),
        analysis.call_references.contains("never_called"),
    )
}

#[test]
fn exec_witness_bypass_families_without_runtime() {
    let prod = "pub fn never_called() -> i32 { 42 }\n";
    let cases: &[(&str, &str, bool, bool, bool)] = &[
        (
            "fn_value_ref",
            "#[cfg(test)]\nmod tests {\n    use super::never_called;\n    #[test]\n    fn witness() { let _ = never_called; }\n}\n",
            true,
            false,
            false,
        ),
        (
            "custom_macro_wrap",
            "macro_rules! cheat { ($t:tt) => { stringify!($t) } }\n#[cfg(test)]\nmod tests {\n    cheat!(never_called);\n    #[test]\n    fn smoke() { assert!(true); }\n}\n",
            true,
            false,
            false,
        ),
        (
            "uninvoked_closure",
            "#[cfg(test)]\nmod tests {\n    use super::never_called;\n    #[test]\n    fn witness() { let _ = || { never_called(); }; }\n}\n",
            true,
            false,
            false,
        ),
        (
            "async_unawaited",
            "#[cfg(test)]\nmod tests {\n    use super::never_called;\n    #[test]\n    fn witness() { let _ = async { never_called(); }; }\n}\n",
            true,
            false,
            false,
        ),
        (
            "uncalled_helper",
            "#[cfg(test)]\nmod tests {\n    use super::never_called;\n    fn witness_farm() { if false { never_called(); } }\n    #[test]\n    fn smoke() { assert!(true); }\n}\n",
            true,
            false,
            false,
        ),
    ];
    for (label, tests, expect_test_refs, expect_call_refs, expect_covered) in cases {
        let (covered, test_refs, call_refs) = bypass_witness_status(&format!("{prod}{tests}"));
        assert_eq!(
            covered, *expect_covered,
            "{label}: executable-witness coverage mismatch"
        );
        assert_eq!(test_refs, *expect_test_refs, "{label}: test_refs mismatch");
        assert_eq!(call_refs, *expect_call_refs, "{label}: call_refs mismatch");
    }
}
