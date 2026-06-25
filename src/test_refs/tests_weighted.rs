use super::*;
use crate::parsing::parse_files;

fn write_branchy_module(path: &std::path::Path, branch_count: usize) {
    let mut body = String::from("def compute(seed: int) -> int:\n    acc = seed\n");
    for i in 0..branch_count {
        body.push_str(&format!(
            "    if seed == {i}:\n        return acc + {i}\n    elif seed == {}:\n        acc += {}\n",
            i + 100,
            i
        ));
    }
    body.push_str("    return acc\n");
    std::fs::write(path, &body).unwrap();
}

fn weighted_pct_for_branchy_module(branch_count: usize) -> usize {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("branchy.py");
    write_branchy_module(&module, branch_count);
    let test_path = tmp.path().join("test_branchy.py");
    std::fs::write(
        &test_path,
        "from branchy import compute\n\ndef test_compute():\n    assert compute(0) >= 0\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path.clone()];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    weighted.get(&module).copied().unwrap_or(100)
}

#[test]
fn weighted_pct_discounts_high_branch_covered_def() {
    let pct = weighted_pct_for_branchy_module(20);
    assert!(
        pct < 100,
        "high-branch covered def should not score 100%, got {pct}%"
    );
}

#[test]
fn weighted_pct_monotone_in_branch_count() {
    let low = weighted_pct_for_branchy_module(2);
    let high = weighted_pct_for_branchy_module(20);
    assert!(
        high <= low,
        "more branches should not increase weighted pct, low={low}% high={high}%"
    );
    assert!(
        high < 100,
        "many-branch module should stay below 100%, got {high}%"
    );
}

#[test]
fn test_weighted_class_import_surface_credit_for_unref_method() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("widget.py");
    std::fs::write(
        &module,
        "class Widget:\n    def covered(self):\n        return 1\n    def heavy(self, n: int) -> int:\n        total = 0\n        for i in range(30):\n            if i == n:\n                total += i\n        return total\n",
    )
    .unwrap();
    let test_path = tmp.path().join("test_widget.py");
    std::fs::write(
        &test_path,
        "from widget import Widget\n\ndef test_widget():\n    w = Widget()\n    assert w.covered() == 1\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path.clone()];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let unref_names: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .filter(|d| d.file == module)
        .map(|d| d.name.as_str())
        .collect();
    assert!(unref_names.contains("heavy"));
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&module).copied().unwrap_or(100);
    assert!(
        pct < 100,
        "unreferenced heavy method should reduce class-weighted pct, got {pct}%"
    );
}

#[test]
fn test_weighted_module_import_surface_for_bind_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("mod.py");
    std::fs::write(
        &module,
        "def entry() -> int:\n    return 1\n\ndef worker(n: int) -> int:\n    return n + 2\n",
    )
    .unwrap();
    let bind_test = tmp.path().join("test_bind.py");
    std::fs::write(
        &bind_test,
        "import mod\n\ndef test_bind_only():\n    fn = mod.worker\n    assert mod.entry() == 1\n",
    )
    .unwrap();
    let call_test = tmp.path().join("test_call.py");
    std::fs::write(
        &call_test,
        "import mod\n\ndef test_calls_worker():\n    assert mod.entry() == 1\n    assert mod.worker(2) == 4\n",
    )
    .unwrap();

    let bind_paths = vec![module.clone(), bind_test.clone()];
    let bind_parsed: Vec<_> = parse_files(&bind_paths)
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let bind_refs: Vec<_> = bind_parsed.iter().collect();
    let bind_analysis = analyze_test_refs(&bind_refs, None);
    assert!(
        !bind_analysis.call_references.contains("worker"),
        "fn bind should not count as a runtime call witness for worker"
    );
    assert!(bind_analysis.call_references.contains("entry"));
    let bind_weighted =
        super::coverage_weighted::compute_py_weighted_file_pcts(&bind_analysis, &bind_refs);
    let bind_pct = bind_weighted.get(&module).copied().unwrap_or(100);

    let call_paths = vec![module.clone(), call_test.clone()];
    let call_parsed: Vec<_> = parse_files(&call_paths)
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    let call_refs: Vec<_> = call_parsed.iter().collect();
    let call_analysis = analyze_test_refs(&call_refs, None);
    assert!(call_analysis.call_references.contains("worker"));
    let call_weighted =
        super::coverage_weighted::compute_py_weighted_file_pcts(&call_analysis, &call_refs);
    let call_pct = call_weighted.get(&module).copied().unwrap_or(0);

    assert!(
        bind_pct < call_pct,
        "bind-only worker should score lower than direct worker call, bind={bind_pct}% call={call_pct}%"
    );
}

#[test]
fn test_weighted_all_called_multi_def_has_high_pct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let module = tmp.path().join("many.py");
    let mut body = String::new();
    for i in 0..4 {
        body.push_str(&format!("def f{i}(n: int) -> int:\n    return n + {i}\n"));
    }
    std::fs::write(&module, &body).unwrap();
    let test_path = tmp.path().join("test_many.py");
    std::fs::write(
        &test_path,
        "from many import f0, f1, f2, f3\n\ndef test_all():\n    assert f0(1) + f1(1) + f2(1) + f3(1) == 10\n",
    )
    .unwrap();
    let paths = vec![module.clone(), test_path.clone()];
    let parsed: Vec<_> = parse_files(&paths).unwrap().into_iter().flatten().collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_test_refs(&refs, None);
    let module_unref: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .filter(|d| d.file == module)
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        module_unref.is_empty(),
        "every def in module should be referenced, unreferenced={module_unref:?}"
    );
    for i in 0..4 {
        let name = format!("f{i}");
        assert!(
            analysis.call_references.contains(name.as_str()),
            "expected call witness for {name}"
        );
    }
    let weighted = super::coverage_weighted::compute_py_weighted_file_pcts(&analysis, &refs);
    let pct = weighted.get(&module).copied().unwrap_or(0);
    assert_eq!(
        pct, 100,
        "branchless fully-witnessed defs should fold to 100%, got {pct}%"
    );
}
