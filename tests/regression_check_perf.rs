use kiss::parsing::{ParsedFile, create_parser};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

fn gen_source_file(idx: usize, func_count: usize) -> (String, PathBuf) {
    let mut code = String::new();
    for f in 0..func_count {
        let _ = writeln!(code, "def func_{idx}_{f}(x):");
        let _ = writeln!(code, "    return x + {f}");
        let _ = writeln!(code);
    }
    (code, PathBuf::from(format!("mod_{idx}.py")))
}

fn gen_test_file(idx: usize, source_count: usize, funcs_per_source: usize) -> (String, PathBuf) {
    let mut code = String::new();
    for s in 0..source_count {
        let _ = writeln!(code, "from mod_{s} import func_{s}_0");
    }
    let _ = writeln!(code);
    for t in 0..funcs_per_source {
        let _ = writeln!(code, "def test_case_{idx}_{t}():");
        for s in 0..source_count {
            let _ = writeln!(code, "    func_{s}_{t}()");
        }
        let _ = writeln!(code);
    }
    (code, PathBuf::from(format!("tests/test_mod_{idx}.py")))
}

/// Regression test: `analyze_test_refs_no_map` (the `kiss check` path) must stay
/// sub-quadratic. With 200 source files × 5 funcs each (1000 definitions) and
/// 20 test files × 5 test funcs (100 tests), the O(tests×defs) coverage map
/// would dominate. The `_no_map` variant must skip that work entirely.
#[test]
fn check_path_skips_coverage_map_and_stays_fast() {
    let mut parser = create_parser().unwrap();
    let source_files = 200;
    let funcs_per_file = 5;
    let test_files = 20;

    let mut parsed: Vec<ParsedFile> = Vec::new();

    for i in 0..source_files {
        let (code, path) = gen_source_file(i, funcs_per_file);
        let tree = parser.parse(&code, None).unwrap();
        parsed.push(ParsedFile {
            path,
            source: code,
            tree,
        });
    }

    for i in 0..test_files {
        let (code, path) = gen_test_file(i, source_files, funcs_per_file);
        let tree = parser.parse(&code, None).unwrap();
        parsed.push(ParsedFile {
            path,
            source: code,
            tree,
        });
    }

    let refs: Vec<&ParsedFile> = parsed.iter().collect();

    let t0 = Instant::now();
    let quick = kiss::analyze_test_refs_no_map(&refs, None);
    let dt_no_map = t0.elapsed();

    assert!(
        quick.coverage_map.is_empty(),
        "no_map variant must return empty coverage_map"
    );
    assert!(
        !quick.definitions.is_empty(),
        "should still produce definitions"
    );

    assert!(
        dt_no_map.as_millis() < 2000,
        "analyze_test_refs_no_map took {}ms — regression: should be <2s for 1000 defs × 100 tests",
        dt_no_map.as_millis(),
    );

    // Sanity: the full version (with coverage_map) is measurably slower.
    let t1 = Instant::now();
    let full = kiss::analyze_test_refs(&refs, None);
    let dt_full = t1.elapsed();

    assert!(
        !full.coverage_map.is_empty(),
        "full variant must produce a non-empty coverage_map"
    );
    assert!(
        dt_full > dt_no_map,
        "full analyze_test_refs ({:.0}ms) should be slower than no_map ({:.0}ms)",
        dt_full.as_secs_f64() * 1000.0,
        dt_no_map.as_secs_f64() * 1000.0,
    );
}
