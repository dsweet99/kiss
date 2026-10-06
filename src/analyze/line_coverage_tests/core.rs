use super::*;

#[test]
fn python_coverage_denominator_includes_match_break_continue() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("flow.py");
    std::fs::write(
        &file,
        "def f(xs):\n    for x in xs:\n        if x:\n            continue\n        break\n    match x:\n        case 1:\n            return 1\n    return 0\n",
    )
    .unwrap();
    let denom = coverage_denominator_lines_for_test(&file).expect("readable python source");
    let source = std::fs::read_to_string(&file).unwrap();
    for (idx, line) in source.lines().enumerate() {
        let n = idx + 1;
        let trimmed = line.trim();
        if trimmed.starts_with("continue")
            || trimmed.starts_with("break")
            || trimmed.starts_with("match ")
            || trimmed.starts_with("case ")
        {
            assert!(
                denom.contains(&n),
                "denominator must include {trimmed} on line {n}: {denom:?}"
            );
        }
    }
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
    let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
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
    let da = coverage_denominator_lines_for_test(&a).expect("readable rust source");
    let db = coverage_denominator_lines_for_test(&b).expect("readable rust source");
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
        let denom = coverage_denominator_lines_for_test(&file).expect("readable rust source");
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
fn coverage_path_does_not_use_host_cfg_test() {
    let src = include_str!("../line_coverage.rs");
    let cfg = include_str!("../line_coverage_cfg.rs");
    assert!(
        !src.contains("cfg!(") && !cfg.contains("cfg!(") && !cfg.contains("std::env::consts::OS"),
        "coverage path must not classify with host cfg! or target OS"
    );
    assert!(
        !cfg.contains("fn cfg_expr_active"),
        "coverage path must not retain the leftover cfg evaluator"
    );
}
