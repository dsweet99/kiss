use super::*;
use kiss::Language;
use std::time::Duration;

#[test]
fn ignore_prefixes_drop_matching_path_selectors() {
    let timings = vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fast/test_a.py::t".into(),
            duration: Duration::from_millis(10),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/slow/test_b.py::t".into(),
            duration: Duration::from_millis(50),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "crate::mod::tests::t".into(),
            duration: Duration::from_millis(20),
        },
    ];
    let filtered = filter_timings_by_ignore(timings, &["tests/slow".into()]);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].selector, "tests/fast/test_a.py::t");
    assert_eq!(filtered[1].selector, "crate::mod::tests::t");

    let mixed = vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fake_python/test_x.py::test_x".into(),
            duration: Duration::from_millis(10),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/test_app.py::test_app".into(),
            duration: Duration::from_millis(10),
        },
    ];
    let filtered = filter_timings_by_ignore(mixed, &["fake_".into()]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].selector, "tests/test_app.py::test_app");

    let rust_logical = vec![
        UnitTestTiming {
            language: Language::Rust,
            selector: "tests::unit_ok".into(),
            duration: Duration::from_secs(2),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "src/lib.rs::unit_ok".into(),
            duration: Duration::from_secs(2),
        },
    ];
    let filtered = filter_timings_by_ignore(rust_logical, &["tests".into()]);
    assert_eq!(filtered.len(), 2);
    let filtered = filter_timings_by_ignore(
        vec![UnitTestTiming {
            language: Language::Rust,
            selector: "src/lib.rs::unit_ok".into(),
            duration: Duration::from_secs(2),
        }],
        &["src".into()],
    );
    assert!(filtered.is_empty());
}
