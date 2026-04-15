use crate::bin_cli::config_session::config_provenance;
use kiss::config_gen::{collect_py_stats_with_ignore, collect_rs_stats_with_ignore};
use kiss::{compute_summaries, format_stats_table, Language, MetricStats};
use std::path::Path;

pub fn run_stats_summary(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    let (mut py_stats, mut rs_stats) = (MetricStats::default(), MetricStats::default());
    let (mut py_cnt, mut rs_cnt) = (0, 0);
    for path in paths {
        let root = Path::new(path);
        if lang_filter.is_none() || lang_filter == Some(Language::Python) {
            let (s, c) = collect_py_stats_with_ignore(root, ignore);
            py_stats.merge(s);
            py_cnt += c;
        }
        if lang_filter.is_none() || lang_filter == Some(Language::Rust) {
            let (s, c) = collect_rs_stats_with_ignore(root, ignore);
            rs_stats.merge(s);
            rs_cnt += c;
        }
    }
    if py_cnt + rs_cnt == 0 {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!(
        "kiss stats - Summary Statistics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    if py_cnt > 0 {
        println!(
            "=== Python ({py_cnt} files) ===\n{}\n",
            format_stats_table(&compute_summaries(&py_stats))
        );
    }
    if rs_cnt > 0 {
        println!(
            "=== Rust ({rs_cnt} files) ===\n{}",
            format_stats_table(&compute_summaries(&rs_stats))
        );
    }
}
