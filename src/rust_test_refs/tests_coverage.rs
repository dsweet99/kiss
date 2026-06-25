use super::compute_rs_weighted_file_pcts;
use crate::rust_parsing::parse_rust_file;
use crate::rust_test_refs::analyze_rust_test_refs;

fn weighted_pct_for_sparse_impl(method_count: usize, test_body: &str) -> usize {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("service.rs");
    let mut body =
        String::from("pub struct Service;\nimpl Service {\n    pub fn new() -> Self { Service }\n");
    for i in 0..method_count {
        body.push_str(&format!(
            "    pub fn method_{i}(seed: u64) -> u64 {{\n        let mut acc = seed;\n        if seed == {i} {{ acc += 1; }} else if seed == {} {{ acc += 2; }}\n        acc\n    }}\n",
            i + 100
        ));
    }
    body.push_str(&format!(
        "}}\n#[cfg(test)]\nmod tests {{\n    use super::Service;\n    {test_body}\n}}\n"
    ));
    std::fs::write(&src, &body).unwrap();

    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    weighted.get(&src).copied().unwrap_or(100)
}

#[test]
fn weighted_pct_monotone_in_unreferenced_method_count() {
    let shallow_test = "#[test]\n    fn only_constructor() { let _ = Service::new(); }";
    let few = weighted_pct_for_sparse_impl(2, shallow_test);
    let many = weighted_pct_for_sparse_impl(10, shallow_test);
    assert!(
        many <= few,
        "more unreferenced methods should not increase weighted pct, few={few}% many={many}%"
    );
    assert!(
        many < 100,
        "sparse shallow coverage should stay below 100%, got {many}%"
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
fn bind_only_fn_pointer_yields_partial_credit_without_call_witness() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    let prod = r#"pub fn shallow() -> i32 { 1 }

pub fn deep(n: i32) -> i32 {
    let mut total = 0;
    for i in 0..300 {
        total += i * n;
    }
    total
}

pub fn gateway(n: i32) -> i32 {
    deep(n)
}
"#;
    let bind_tests = r#"#[cfg(test)]
mod tests {
    use super::{gateway, deep, shallow};

    #[test]
    fn test_shallow_runs() {
        assert_eq!(shallow(), 1);
    }

    #[test]
    fn test_gateway_bind_only() {
        let _fp: fn(i32) -> i32 = gateway;
        let _ = deep as fn(i32) -> i32;
    }
}
"#;
    let call_tests = r#"#[cfg(test)]
mod tests {
    use super::{gateway, shallow};

    #[test]
    fn test_shallow_runs() {
        assert_eq!(shallow(), 1);
    }

    #[test]
    fn test_gateway_called() {
        assert!(gateway(1) > 0);
    }
}
"#;
    std::fs::write(&src, format!("{prod}{bind_tests}")).unwrap();
    let bind_parsed = parse_rust_file(&src).unwrap();
    let bind_refs: Vec<_> = [&bind_parsed].into_iter().collect();
    let bind_analysis = analyze_rust_test_refs(&bind_refs, None);
    let bind_weighted = compute_rs_weighted_file_pcts(&bind_analysis, &bind_refs);
    let bind_pct = bind_weighted.get(&src).copied().unwrap_or(100);

    std::fs::write(&src, format!("{prod}{call_tests}")).unwrap();
    let call_parsed = parse_rust_file(&src).unwrap();
    let call_refs: Vec<_> = [&call_parsed].into_iter().collect();
    let call_analysis = analyze_rust_test_refs(&call_refs, None);
    let call_weighted = compute_rs_weighted_file_pcts(&call_analysis, &call_refs);
    let call_pct = call_weighted.get(&src).copied().unwrap_or(0);

    let bind_unref: Vec<_> = bind_analysis
        .unreferenced
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        bind_unref.contains(&"gateway") && bind_unref.contains(&"deep"),
        "fn-pointer binds should leave gateway/deep unreferenced, got {bind_unref:?} call_refs={:?}",
        bind_analysis.call_references
    );
    assert!(
        !bind_unref.contains(&"shallow"),
        "shallow has a live call witness and must stay referenced"
    );
    assert_eq!(
        bind_pct, 100,
        "only shallow is referenced under executable witnesses; weighted pct counts referenced mass only, got {bind_pct}%"
    );
    assert!(
        !bind_analysis.call_references.contains("deep"),
        "fn-pointer bind should not count as a runtime call witness for deep"
    );
    assert!(
        call_analysis.call_references.contains("gateway")
            || call_analysis.test_references.contains("deep"),
        "direct gateway call should reference deep transitively"
    );
    assert!(
        call_pct >= bind_pct,
        "direct call should not score below bind-only, bind={bind_pct}% call={call_pct}%"
    );
}

#[test]
fn syntactic_scope_collects_refs_inside_const_false_test_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    std::fs::write(
        &src,
        r#"pub fn covered() -> i32 {
    1
}

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
    fn dead_branch_only() {
        if false {
            let _ = unreached();
        }
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
    assert!(
        unref_names.contains("unreached"),
        "const-false branches never execute; unreached should be unreferenced"
    );
    assert!(!unref_names.contains("covered"));
    assert!(
        !analysis.call_references.contains("unreached"),
        "dead-branch calls must not count as executable witnesses"
    );
}

#[test]
fn weighted_pct_pub_use_export_penalizes_unref() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    std::fs::write(
        &src,
        r#"pub fn covered() -> i32 { 1 }

pub fn uncovered() -> i32 {
    let mut total = 0;
    for i in 0..50 {
        total += i;
    }
    total
}

pub use covered;
pub use uncovered;

#[cfg(test)]
mod tests {
    use super::covered;

    #[test]
    fn test_covered() {
        assert_eq!(covered(), 1);
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
    assert!(unref_names.contains("uncovered"));
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&src).copied().unwrap_or(100);
    assert!(
        pct < 100,
        "pub use of uncovered fn should reduce weighted pct, got {pct}%"
    );
}

#[test]
fn weighted_pct_impl_import_surface_below_full_credit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("service.rs");
    std::fs::write(
        &src,
        r#"pub struct Service;

impl Service {
    pub fn new() -> Self { Service }
    pub fn heavy_a(n: u64) -> u64 {
        let mut acc = n;
        for i in 0..20 { if i == n { acc += 1; } }
        acc
    }
    pub fn heavy_b(n: u64) -> u64 {
        let mut acc = n;
        for i in 0..20 { if i == n { acc += 2; } }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::Service;
    #[test]
    fn only_constructor() { let _ = Service::new(); }
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
    assert!(unref_names.contains("heavy_a"));
    assert!(unref_names.contains("heavy_b"));
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&src).copied().unwrap_or(100);
    assert!(
        pct < 100,
        "impl import-surface credit should stay below 100%, got {pct}%"
    );
}

#[test]
fn weighted_pct_locates_fn_in_nested_mod() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("nested.rs");
    std::fs::write(
        &src,
        r#"mod inner {
    pub fn compute(n: i32) -> i32 {
        let mut total = 0;
        for i in 0..30 { total += i * n; }
        total
    }
}

pub fn entry(n: i32) -> i32 {
    inner::compute(n)
}

#[cfg(test)]
mod tests {
    use super::entry;
    #[test]
    fn calls_entry() { assert!(entry(1) >= 0); }
}
"#,
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let unref_names: Vec<_> = analysis
        .unreferenced
        .iter()
        .filter(|d| d.file == src)
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        unref_names.is_empty(),
        "entry() call transitively executes inner::compute; both should be referenced, unref={unref_names:?}"
    );
    assert!(
        analysis.call_references.contains("compute") || analysis.call_references.contains("entry"),
        "transitive production call refs should witness nested compute"
    );
}
