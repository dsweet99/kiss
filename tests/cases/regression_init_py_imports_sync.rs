use std::fs;
use std::process::Command;
use tempfile::TempDir;

use kiss::parsing::parse_files;
use kiss::{MetricStats, compute_summaries};

fn write_init_only_corpus(dir: &std::path::Path) -> std::path::PathBuf {
    let pkg = dir.join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("__init__.py"),
        "import os\nimport sys\nimport json\nfrom typing import Dict, List, Optional, Set, Tuple\n",
    )
    .unwrap();
    pkg.join("__init__.py")
}

#[test]
fn metric_stats_collect_skips_init_py_for_imported_names() {
    let tmp = TempDir::new().unwrap();
    let init_path = write_init_only_corpus(tmp.path());

    let parsed = parse_files(&[init_path])
        .expect("parse_files should succeed")
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(parsed.len(), 1, "expected to parse exactly one file");

    let parsed_refs: Vec<_> = parsed.iter().collect();
    let stats = MetricStats::collect(&parsed_refs);

    assert!(
        stats.imported_names_per_file.is_empty(),
        "MetricStats::collect must skip __init__.py for imported_names_per_file \
         (kiss check and `kiss stats --all` both skip it; the summary path must \
         agree per grounding M6). Got: {:?}",
        stats.imported_names_per_file
    );

    let summaries = compute_summaries(&stats);
    let imports_summary = summaries
        .iter()
        .find(|s| s.metric_id == "imported_names_per_file");
    assert!(
        imports_summary.is_none(),
        "compute_summaries must not yield an imported_names_per_file row when \
         every parsed file is __init__.py; got {imports_summary:?}"
    );
}

#[test]
fn kiss_stats_summary_excludes_init_py_imports() {
    let tmp = TempDir::new().unwrap();
    write_init_only_corpus(tmp.path());

    let home = TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("stats")
        .arg(tmp.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss stats should run");
    assert!(
        out.status.success(),
        "kiss stats failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    let imports_row = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("imported_names_per_file"));

    if let Some(row) = imports_row {
        let nums: Vec<usize> = row
            .split_whitespace()
            .skip(1)
            .filter_map(|tok| tok.parse::<usize>().ok())
            .collect();
        assert!(
            !nums.is_empty(),
            "imported_names_per_file row had no numeric columns:\n{row}\n\
             full output:\n{stdout}"
        );
        assert!(
            nums.iter().all(|&v| v == 0),
            "kiss stats summary reports nonzero imported_names_per_file in an \
             __init__.py-only corpus, but kiss check skips __init__.py for this \
             metric (grounding M6 sync break). Row:\n  {row}\nFull output:\n{stdout}"
        );
    }
}
