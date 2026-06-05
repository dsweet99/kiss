use super::compute_rs_weighted_file_pcts;
use crate::rust_parsing::parse_rust_file;
use crate::rust_test_refs::analyze_rust_test_refs;

fn weighted_pct_for_worker(task_count: usize, test_body: &str) -> usize {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    let mut body = String::from(
        "pub struct Worker;\nimpl Worker {\n    pub fn new() -> Self { Worker }\n",
    );
    for i in 0..task_count {
        body.push_str(&format!(
            "    pub fn task_{i}(seed: u64) -> u64 {{\n        let mut acc = seed;\n        if seed == {i} {{ acc += 1; }} else if seed == {} {{ acc += 2; }}\n        acc\n    }}\n",
            i + 100
        ));
    }
    body.push_str(&format!(
        "}}\n#[cfg(test)]\nmod tests {{\n    use super::Worker;\n    {test_body}\n}}\n"
    ));
    std::fs::write(&src, &body).unwrap();

    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    weighted.get(&src).copied().unwrap_or(100)
}

#[test]
fn weighted_pct_low_for_new_only_worker() {
    let pct = weighted_pct_for_worker(10, "#[test]\n    fn worker_new_only() { let _ = Worker::new(); }");
    assert!(
        pct < 100,
        "new-only worker should not score 100%, got {pct}%"
    );
}

#[test]
fn weighted_pct_monotone_in_task_count() {
    let few = weighted_pct_for_worker(2, "#[test]\n    fn worker_new_only() { let _ = Worker::new(); }");
    let many = weighted_pct_for_worker(10, "#[test]\n    fn worker_new_only() { let _ = Worker::new(); }");
    assert!(
        many <= few,
        "more unreferenced tasks should not increase weighted pct, few={few}% many={many}%"
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
fn bind_only_fn_pointer_leaves_heavy_routine_unreferenced() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    std::fs::write(
        &src,
        r#"pub fn trivial() -> i32 { 1 }

pub fn heavy_routine(n: i32) -> i32 {
    let mut total = 0;
    for i in 0..300 {
        total += i * n;
    }
    total
}

pub fn entry_point(n: i32) -> i32 {
    heavy_routine(n)
}

#[cfg(test)]
mod tests {
    use super::{entry_point, heavy_routine, trivial};

    #[test]
    fn test_trivial_runs() {
        assert_eq!(trivial(), 1);
    }

    #[test]
    fn test_entry_bind_only() {
        let _fp: fn(i32) -> i32 = entry_point;
        let _ = heavy_routine as fn(i32) -> i32;
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
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&parsed.path).copied().unwrap_or(100);
    assert!(
        pct < 100,
        "bind-only fn pointers should not yield full weighted credit, got {pct}% unref={unref_names:?}"
    );
    assert!(
        !analysis.call_references.contains("heavy_routine"),
        "fn-pointer bind should not count as a runtime call witness for heavy_routine"
    );
}

#[test]
fn const_false_test_body_leaves_defs_unreferenced() {
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
    assert!(unref_names.contains("unreached"));
    assert!(!unref_names.contains("covered"));
    assert!(
        !analysis.call_references.contains("unreached"),
        "const-false body should not count as a runtime call witness"
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
fn weighted_pct_impl_type_import_surface_credit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("worker.rs");
    std::fs::write(
        &src,
        r#"pub struct Worker;

impl Worker {
    pub fn new() -> Self { Worker }
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
    use super::Worker;
    #[test]
    fn only_new() { let _ = Worker::new(); }
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
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&src).copied().unwrap_or(100);
    let unref_names: Vec<_> = analysis
        .unreferenced
        .iter()
        .filter(|d| d.file == src)
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        pct < 100,
        "nested heavy fn with shallow entry test should not score 100%, got {pct}% unref={unref_names:?}"
    );
}
