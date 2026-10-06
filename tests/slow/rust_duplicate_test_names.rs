#![cfg(unix)]

use crate::support::scenario::{Reply, Scenario, kiss, skip_under_coverage};

fn assert_summary(reply: &Reply, code: i32, summary: &str, phase: &str) {
    assert_eq!(reply.code, Some(code), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), summary, "{phase}: {reply:?}");
}

fn status_lines(reply: &Reply) -> Vec<String> {
    let mut lines: Vec<String> = reply
        .stdout
        .lines()
        .filter(|line| line.starts_with("PASS") || line.starts_with("FAIL"))
        .map(|line| line.split(" (").next().unwrap_or(line).to_string())
        .collect();
    lines.sort();
    lines
}

fn marked_test(s: &Scenario, label: &str, name: &str, body: &str) -> String {
    format!(
        "#[test]\nfn {name}() {{\n    use std::io::Write;\n    let mut fh = std::fs::OpenOptions::new().create(true).append(true).open({:?}).unwrap();\n    writeln!(fh, \"{label}\").unwrap();\n    {body}\n}}\n",
        s.marker().to_str().unwrap()
    )
}

fn demo_crate(s: &Scenario, lib: &str) {
    s.write(".gitignore", ".kiss/\ntarget/\n");
    crate::support::scenario::write_kissconfig_with_threshold(s.root(), 0);
    s.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    s.write("src/lib.rs", lib);
}

fn integration_file(s: &Scenario, file: &str, same_body: &str, ok_name: &str) -> String {
    let label = file.trim_end_matches(".rs");
    format!(
        "{}\n{}",
        marked_test(s, &format!("{label}::t_same"), "t_same", same_body),
        marked_test(
            s,
            &format!("{label}::{ok_name}"),
            ok_name,
            "assert_eq!(demo::value(), 7);"
        )
    )
}

#[test]
fn same_named_integration_tests_in_two_files_are_two_tests() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    demo_crate(&s, "pub fn value() -> u32 {\n    7\n}\n");
    s.write(
        "tests/a.rs",
        &integration_file(&s, "a.rs", "assert_eq!(demo::value(), 1);", "t_ok_a"),
    );
    s.write(
        "tests/b.rs",
        &integration_file(&s, "b.rs", "assert_eq!(demo::value(), 2);", "t_ok_b"),
    );
    crate::common::generate_lockfile(s.root());
    s.commit();
    let broken = "✗ 2 passed · 2 failed · 0 timed out";

    let first = kiss(s.root(), &["test"]);
    assert_summary(&first, 1, broken, "first run");
    assert_eq!(
        status_lines(&first),
        [
            "FAIL: tests/a.rs::t_same",
            "FAIL: tests/b.rs::t_same",
            "PASS: tests/a.rs::t_ok_a",
            "PASS: tests/b.rs::t_ok_b",
        ],
        "every test has its own line, attributed to its own file: {first:?}"
    );
    assert_eq!(
        s.take_runs(),
        ["a::t_ok_a", "a::t_same", "b::t_ok_b", "b::t_same"]
    );

    let cached = kiss(s.root(), &["test"]);
    assert_summary(&cached, 1, broken, "cached run");
    let cached_fails: Vec<String> = status_lines(&cached)
        .into_iter()
        .filter(|line| line.starts_with("FAIL"))
        .collect();
    assert_eq!(cached_fails.len(), 2, "both FAILs are listed: {cached:?}");
    assert!(
        cached_fails.iter().any(|line| line.contains("tests/a.rs"))
            && cached_fails.iter().any(|line| line.contains("tests/b.rs")),
        "each cached FAIL names its own file: {cached:?}"
    );
    assert!(s.take_runs().is_empty(), "no edit, nothing reruns");

    s.write(
        "tests/a.rs",
        &integration_file(&s, "a.rs", "assert_eq!(demo::value(), 7);", "t_ok_a"),
    );
    let fixed_a = kiss(s.root(), &["test"]);
    assert_summary(
        &fixed_a,
        1,
        "✗ 3 passed · 1 failed · 0 timed out",
        "after fixing a.rs",
    );
    assert!(
        fixed_a.stdout.contains("tests/b.rs::t_same")
            && !fixed_a.stdout.contains("FAIL: tests/a.rs"),
        "only b.rs's t_same still fails: {fixed_a:?}"
    );
    assert!(
        s.take_runs().iter().any(|name| name == "a::t_same"),
        "the edited file's test reruns"
    );
}

fn same_named_unit_tests(s: &Scenario) {
    for (module, want) in [("a", 1), ("b", 7)] {
        let test = marked_test(
            s,
            &format!("{module}::same"),
            "same",
            &format!("assert_eq!(super::value(), {want});"),
        );
        s.write(
            &format!("src/{module}.rs"),
            &format!(
                "pub fn value() -> u32 {{\n    7\n}}\n\n#[cfg(test)]\nmod tests {{\n{test}}}\n"
            ),
        );
    }
    crate::common::generate_lockfile(s.root());
    s.commit();
}

fn assert_two_unit_tests(s: &Scenario, runs: &[&str]) {
    let summary = "✗ 1 passed · 1 failed · 0 timed out";

    let first = kiss(s.root(), &["test"]);
    assert_summary(&first, 1, summary, "first run");
    assert_eq!(
        status_lines(&first),
        ["FAIL: src/a.rs::same", "PASS: src/b.rs::same"],
        "the FAIL belongs to src/a.rs and src/b.rs's test passes: {first:?}"
    );
    assert_eq!(s.take_runs(), runs);

    let cached = kiss(s.root(), &["test"]);
    assert_summary(&cached, 1, summary, "cached run");
    assert!(s.take_runs().is_empty(), "no edit, nothing reruns");
}

#[test]
fn same_named_unit_tests_in_two_modules_are_two_tests() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    demo_crate(&s, "pub mod a;\npub mod b;\n");
    same_named_unit_tests(&s);
    assert_two_unit_tests(&s, &["a::same", "b::same"]);
}

#[test]
fn same_named_unit_tests_in_modules_shared_by_lib_and_bin_are_two_tests() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    demo_crate(&s, "pub mod a;\npub mod b;\n");
    s.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"tool\"\npath = \"src/main.rs\"\n",
    );
    s.write("src/main.rs", "mod a;\nmod b;\n\nfn main() {}\n");
    same_named_unit_tests(&s);
    assert_two_unit_tests(&s, &["a::same", "a::same", "b::same", "b::same"]);
}
