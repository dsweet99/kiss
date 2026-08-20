use super::*;
use kiss::Language;
use std::time::Duration;

#[test]
fn grouped_runtime_report_partitions_by_first_matching_rule() {
    let rules = vec![
        ("tests/slow/dbs".to_string(), 180.0),
        ("tests/slow".to_string(), 60.0),
        ("tests/fast".to_string(), 2.0),
        ("tests/".to_string(), 10.0),
        ("rust".to_string(), 10.0),
        ("*".to_string(), 0.0),
    ];
    let timings = vec![
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/slow/dbs/query.py::test_q".into(),
            duration: Duration::from_millis(100),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/slow/other.py::test_o".into(),
            duration: Duration::from_millis(200),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/fast/a.py::test_a".into(),
            duration: Duration::from_millis(10),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "tests/web/b.py::test_b".into(),
            duration: Duration::from_millis(40),
        },
        UnitTestTiming {
            language: Language::Rust,
            selector: "rust/crate/src/lib.rs::test_lib".into(),
            duration: Duration::from_millis(50),
        },
        UnitTestTiming {
            language: Language::Python,
            selector: "src/app.py::test_app".into(),
            duration: Duration::from_millis(5),
        },
    ];
    let report = build_unit_test_runtime_grouped_report(&timings, &rules, Some(99)).unwrap();
    assert_eq!(report.rows.len(), 6);
    assert_eq!(report.codebase_tests, Some(99));
    assert_eq!(report.rows[0].pattern, "tests/slow/dbs");
    assert_eq!(report.rows[0].sample_count, 1);
    assert_eq!(report.rows[0].max_ms, Some(100));
    assert_eq!(report.rows[1].pattern, "tests/slow");
    assert_eq!(report.rows[1].sample_count, 1);
    assert_eq!(report.rows[1].max_ms, Some(200));
    assert_eq!(report.rows[2].pattern, "tests/fast");
    assert_eq!(report.rows[2].sample_count, 1);
    assert_eq!(report.rows[3].pattern, "tests/");
    assert_eq!(report.rows[3].sample_count, 1);
    assert_eq!(report.rows[3].max_ms, Some(40));
    assert_eq!(report.rows[4].pattern, "rust");
    assert_eq!(report.rows[4].sample_count, 1);
    assert_eq!(report.rows[4].max_ms, Some(50));
    assert_eq!(report.rows[5].pattern, "*");
    assert_eq!(report.rows[5].sample_count, 1);
    assert_eq!(report.rows[5].max_ms, Some(5));
    assert_eq!(
        report.rows.iter().map(|r| r.sample_count).sum::<usize>(),
        timings.len()
    );

    let formatted = format_unit_test_runtime_grouped_report(&report);
    assert!(formatted.starts_with(
        "unit_test_runtime_sec: (coverage cache; may not reflect full test set) codebase_tests=99\n"
    ));
    assert!(formatted.contains("pattern"));
    assert!(formatted.contains("tests/slow/dbs"));
    assert!(formatted.contains("tests/fast"));
    assert!(!formatted.contains('\t'));
}

#[test]
fn grouped_runtime_report_keeps_empty_rows_and_handles_defaults() {
    let multi = vec![("tests/slow".to_string(), 60.0), ("*".to_string(), 0.0)];
    let report = build_unit_test_runtime_grouped_report(
        &[UnitTestTiming {
            language: Language::Python,
            selector: "tests/slow/a.py::t".into(),
            duration: Duration::from_millis(30),
        }],
        &multi,
        None,
    )
    .unwrap();
    assert_eq!(report.rows[0].sample_count, 1);
    assert_eq!(report.rows[1].sample_count, 0);
    assert_eq!(report.rows[1].p50_ms, None);
    let formatted = format_unit_test_runtime_grouped_report(&report);
    assert!(formatted.lines().any(|line| {
        line.split_whitespace()
            .eq(["*", "0", "0", "-", "-", "-", "-", "-"])
    }));

    let sole_star =
        build_unit_test_runtime_grouped_report(&[], &[("*".into(), 2.0)], None).unwrap();
    assert_eq!(sole_star.rows.len(), 1);
    assert_eq!(sole_star.rows[0].sample_count, 0);

    assert!(build_unit_test_runtime_grouped_report(&[], &[], None).is_none());
}

#[test]
fn grouped_runtime_report_right_aligns_each_numeric_column() {
    let report = UnitTestRuntimeGroupedReport {
        codebase_tests: None,
        rows: vec![
            UnitTestRuntimeGroupRow {
                pattern: "tests/slow".into(),
                limit_seconds: 90.0,
                sample_count: 512,
                p50_ms: Some(810),
                p90_ms: Some(14_740),
                p95_ms: Some(27_760),
                p99_ms: Some(61_900),
                max_ms: Some(83_240),
            },
            UnitTestRuntimeGroupRow {
                pattern: "tests/fast".into(),
                limit_seconds: 4.0,
                sample_count: 13_286,
                p50_ms: Some(20),
                p90_ms: Some(290),
                p95_ms: Some(1_110),
                p99_ms: Some(2_690),
                max_ms: Some(3_970),
            },
        ],
    };

    let formatted = format_unit_test_runtime_grouped_report(&report);
    let table: Vec<&str> = formatted.lines().skip(1).collect();
    let header_ends = whitespace_delimited_cell_ends(table[0]);
    assert_eq!(header_ends.len(), 8);
    for row in &table[1..] {
        assert_eq!(&whitespace_delimited_cell_ends(row)[1..], &header_ends[1..]);
    }
}

fn whitespace_delimited_cell_ends(line: &str) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut in_cell = false;
    for (index, character) in line.char_indices() {
        if character.is_whitespace() {
            if in_cell {
                ends.push(index);
                in_cell = false;
            }
        } else {
            in_cell = true;
        }
    }
    if in_cell {
        ends.push(line.len());
    }
    ends
}
