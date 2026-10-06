use std::collections::BTreeSet;

use super::covered_items_digest;

const BASE: &str = "pub const K: i32 = 7;\n\npub fn f(x: i32) -> i32 {\n    x + K\n}\n\npub fn g(x: i32) -> i32 {\n    x * 2\n}\n";

fn covered_f() -> BTreeSet<u32> {
    BTreeSet::from([4])
}

#[test]
fn edit_to_uncovered_function_body_keeps_digest() {
    let edited = BASE.replace("x * 2", "x * 3");
    assert_eq!(
        covered_items_digest(BASE, &covered_f()),
        covered_items_digest(&edited, &covered_f())
    );
}

#[test]
fn edit_to_covered_function_body_changes_digest() {
    let edited = BASE.replace("x + K", "x - K");
    assert_ne!(
        covered_items_digest(BASE, &covered_f()),
        covered_items_digest(&edited, &covered_f())
    );
}

#[test]
fn edit_to_const_outside_function_bodies_changes_digest() {
    let edited = BASE.replace("= 7;", "= 8;");
    assert_ne!(
        covered_items_digest(BASE, &covered_f()),
        covered_items_digest(&edited, &covered_f())
    );
}

#[test]
fn shifting_lines_keeps_digest() {
    let shifted = format!("\n\n{BASE}");
    assert_eq!(
        covered_items_digest(BASE, &covered_f()),
        covered_items_digest(&shifted, &BTreeSet::from([6]))
    );
}

#[test]
fn unparsable_text_digests_whole_text() {
    let broken = "fn f( {";
    assert_ne!(
        covered_items_digest(broken, &covered_f()),
        covered_items_digest("fn f( {}", &covered_f())
    );
}

const TESTS: &str = "use super::*;\n\n#[test]\nfn t7() {\n    assert_eq!(f7(1), 8);\n}\n\n#[test]\nfn t8() {\n    assert_eq!(f8(1), 9);\n}\n";

#[test]
fn test_definition_digest_moves_only_with_its_own_test() {
    let before = super::test_definition_digests("src/tests.rs", TESTS);
    let after =
        super::test_definition_digests("src/tests.rs", &TESTS.replace("f8(1), 9", "f8(1), 10"));
    assert_eq!(before.len(), 3);
    assert_eq!(before["src/tests.rs::t7"], after["src/tests.rs::t7"]);
    assert_ne!(before["src/tests.rs::t8"], after["src/tests.rs::t8"]);
    assert_eq!(before["src/tests.rs::"], after["src/tests.rs::"]);
}

#[test]
fn test_support_digest_moves_with_non_test_items_in_the_test_file() {
    let with_helper = format!("{TESTS}\nfn helper() -> i32 {{\n    8\n}}\n");
    let before = super::test_definition_digests("src/tests.rs", &with_helper);
    let after =
        super::test_definition_digests("src/tests.rs", &with_helper.replace("    8\n", "    9\n"));
    assert_ne!(before["src/tests.rs::"], after["src/tests.rs::"]);
    assert_eq!(before["src/tests.rs::t7"], after["src/tests.rs::t7"]);
}

#[test]
fn selector_leaf_strips_module_and_binary_prefixes() {
    assert_eq!(super::selector_leaf("t7"), "t7");
    assert_eq!(super::selector_leaf("tests::t7"), "t7");
    assert_eq!(super::selector_leaf("pkg::bin$mod_a::alpha"), "alpha");
}

#[test]
fn coverage_excluded_matches_export_ignore_patterns() {
    use super::is_coverage_excluded;
    for rel in [
        "tests/it.rs",
        "tests/common/mod.rs",
        "crates/a/benches/b.rs",
        "examples/e.rs",
        "src/tests.rs",
        "src/foo_tests.rs",
        "src/foo-tests.rs",
    ] {
        assert!(is_coverage_excluded(rel), "{rel}");
    }
    for rel in [
        "src/lib.rs",
        "src/foo_test.rs",
        "src/_tests.rs",
        "src/mytests.rs",
    ] {
        assert!(!is_coverage_excluded(rel), "{rel}");
    }
}
