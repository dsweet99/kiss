use super::*;

#[test]
fn computes_physical_line_percent_and_first_uncovered() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("app.py");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "a\nb\nc\n").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::from([("src/app.py".to_string(), BTreeSet::from([0, 1, 3, 9]))]),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert_eq!(record.total_lines, 3);
    assert_eq!(record.covered_lines, 2);
    assert_eq!(record.percent, 67);
    assert_eq!(record.first_uncovered_line, Some(2));
}

#[test]
fn empty_file_is_fully_covered() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("empty.py");
    std::fs::write(&file, "").unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert_eq!(record.percent, 100);
    assert_eq!(record.first_uncovered_line, None);
}

#[test]
fn python_coverage_denominator_ignores_non_coverable_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("shim.py");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "#!/usr/bin/env python3\n\
             \"\"\"Small wrapper.\"\"\"\n\
             \n\
             from __future__ import annotations\n\
             \n\
             import sys\n\
             \n\
             \n\
             def main() -> None:\n\
                 return None\n\
             \n\
             \n\
             if __name__ == \"__main__\":\n\
                 main()\n",
    )
    .unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::from([(
            "src/shim.py".to_string(),
            BTreeSet::from([2, 4, 6, 9, 10, 13, 14]),
        )]),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert_eq!(record.total_lines, 7);
    assert_eq!(record.covered_lines, 7);
    assert_eq!(record.percent, 100);
}

#[test]
fn rust_coverage_denominator_ignores_declarations_without_runtime_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "use std::fmt;\n\
             \n\
             pub struct Thing;\n\
             \n\
             pub fn run() -> i32 {\n\
                 let x = 1;\n\
                 x\n\
             }\n",
    )
    .unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([5, 6, 7]))]),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert_eq!(record.total_lines, 3);
    assert_eq!(record.covered_lines, 3);
    assert_eq!(record.percent, 100);
}

#[test]
fn rust_coverage_denominator_skips_coverage_off_functions() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("fork.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "pub fn tracked() -> i32 {\n\
             1\n\
         }\n\
         \n\
         #[doc = \"kiss-coverage-off\"]\n\
         pub fn untracked() -> i32 {\n\
             let x = 1;\n\
             x + 1\n\
         }\n",
    )
    .unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::from([("src/fork.rs".to_string(), BTreeSet::from([1, 2]))]),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert_eq!(record.total_lines, 2);
    assert_eq!(record.covered_lines, 2);
    assert_eq!(record.percent, 100);
}

#[test]
fn rust_coverage_denominator_uses_statement_start_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("defaults.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "pub fn text() -> String {\n\
                 format!(\n\
                     \"{} {}\",\n\
                     \"hello\",\n\
                     \"world\",\n\
                 )\n\
             }\n",
    )
    .unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::from([("src/defaults.rs".to_string(), BTreeSet::from([1, 2]))]),
    };

    let record = compute_file_line_coverage(tmp.path(), &file, &snapshot);

    assert_eq!(record.total_lines, 2);
    assert_eq!(record.covered_lines, 2);
    assert_eq!(record.percent, 100);
}

#[test]
fn rust_coverage_denominator_skips_attribute_and_bare_else_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("src").join("gated.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "pub fn pick(flag: bool) -> i32 {\n\
                 if flag {\n\
                     1\n\
                 } else {\n\
                     0\n\
                 }\n\
                 #[cfg(unix)]\n\
                 {\n\
                     let _ = 1;\n\
                 }\n\
                 2\n\
             }\n",
    )
    .unwrap();
    let denom = coverage_denominator_lines(&file).expect("readable rust source");
    let source = std::fs::read_to_string(&file).unwrap();
    for (idx, line) in source.lines().enumerate() {
        let n = idx + 1;
        let trimmed = line.trim();
        if trimmed.starts_with("#[") || trimmed == "else {" || trimmed == "} else {" {
            assert!(
                !denom.contains(&n),
                "denominator must skip unattributable line {n}: {trimmed}"
            );
        }
    }
    assert!(denom.contains(&1));
    assert!(denom.contains(&3) || denom.contains(&2));
}

#[test]
fn metamorphic_rust_denominator_attribute_skip_stable_under_spacing() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.rs");
    let b = tmp.path().join("b.rs");
    std::fs::write(
        &a,
        "pub fn f() {\n    #[cfg(unix)]\n    {\n        let x = 1;\n        let _ = x;\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        &b,
        "pub fn f() {\n    #[cfg(unix)]\n    {\n        let x = 1;\n        let _ = x;\n    }\n}\n",
    )
    .unwrap();
    let da = coverage_denominator_lines(&a).expect("readable rust source");
    let db = coverage_denominator_lines(&b).expect("readable rust source");
    assert_eq!(da.len(), db.len());
    for lines in [&da, &db] {
        for n in lines {
            let text = std::fs::read_to_string(&a).unwrap();
            let row = text.lines().nth(n - 1).unwrap().trim();
            assert!(!row.starts_with("#["));
        }
    }
}

#[test]
fn fuzz_rust_denominator_never_counts_attribute_only_lines() {
    let seed = 0xdec0_de70_u64;
    println!("fuzz_rust_denominator_never_counts_attribute_only_lines seed={seed}");
    let mut rng = seed;
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..32 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let flag = rng.is_multiple_of(2);
        let body = if flag {
            "#[cfg(unix)]\n    {\n        let y = 2;\n        let _ = y;\n    }\n"
        } else {
            "let y = 2;\n    let _ = y;\n"
        };
        let file = tmp.path().join(format!("f{i}.rs"));
        std::fs::write(&file, format!("pub fn g() {{\n    {body}}}\n")).unwrap();
        let denom = coverage_denominator_lines(&file).expect("readable rust source");
        let text = std::fs::read_to_string(&file).unwrap();
        for n in &denom {
            let row = text.lines().nth(n - 1).unwrap().trim();
            assert!(
                !row.starts_with("#["),
                "seed={seed} iter={i} counted attribute line {n}"
            );
        }
    }
}

#[test]
fn cfg_test_only_scan_skips_inline_module_items_without_file_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    std::fs::write(
            &lib,
            "mod inline {\n    pub fn helper() {}\n}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n",
        )
        .unwrap();
    let snapshot = RuntimeCoverageSnapshot {
        identity: "id".to_string(),
        covered_lines: BTreeMap::new(),
    };

    let records =
        compute_line_coverage_records(tmp.path(), &[], std::slice::from_ref(&lib), &snapshot);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].file, lib);
}
