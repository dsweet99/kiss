use kiss::GateConfig;
use std::path::{Path, PathBuf};

pub(super) struct SnapshotExtras {
    pub orphan: usize,
    covered: usize,
    coverable: usize,
}

pub(super) fn try_snapshot_extras(
    paths: &[String],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    ignore: &[String],
    gate: &GateConfig,
) -> Option<SnapshotExtras> {
    if py_files.is_empty() && rs_files.is_empty() {
        return None;
    }
    let universe = Path::new(paths.first().map(String::as_str).unwrap_or("."));
    let repo_root = crate::test_runner::check_line_coverage::repository_root_for_universe(universe);
    let required = crate::test_runner::check_line_coverage::RequiredCoverageLanguages {
        python: !py_files.is_empty(),
        rust: !rs_files.is_empty(),
    };
    let pytest_args = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    let snapshot = crate::test_runner::check_line_coverage::load_check_runtime_coverage(
        &repo_root,
        required,
        ignore,
        gate,
        &pytest_args,
    )
    .ok()?;
    let viols = crate::analyze::collect_orphan_unit_violations(
        &repo_root,
        py_files,
        rs_files,
        &snapshot,
        &gate.orphan_allowed,
    )
    .ok()?;
    let facts =
        crate::analyze::line_coverage::CoverageSourceFacts::from_files(py_files, rs_files).ok()?;
    let records = crate::analyze::line_coverage::records_from_denoms(
        &repo_root,
        &facts.production_denoms(),
        &snapshot,
    );
    Some(SnapshotExtras {
        orphan: viols.len(),
        covered: records.iter().map(|record| record.covered_lines).sum(),
        coverable: records.iter().map(|record| record.total_lines).sum(),
    })
}

pub(super) fn format_coverage_counts(snapshot: &SnapshotExtras) -> String {
    let percent = if snapshot.coverable == 0 {
        100
    } else {
        crate::analyze::line_coverage::coverage_percentage(snapshot.covered, snapshot.coverable)
    };
    format!(
        "Coverage: {} covered, {} coverable ({}%)",
        snapshot.covered, snapshot.coverable, percent
    )
}

#[cfg(test)]
mod summary_snapshot_test {
    use super::{SnapshotExtras, format_coverage_counts, try_snapshot_extras};
    use kiss::GateConfig;

    #[test]
    fn empty_files_yield_no_snapshot() {
        assert!(
            try_snapshot_extras(&[".".into()], &[], &[], &[], &GateConfig::default()).is_none()
        );
        let missing = std::path::PathBuf::from("missing_for_snapshot.py");
        let _ = try_snapshot_extras(&[".".into()], &[missing], &[], &[], &GateConfig::default());
    }

    #[test]
    fn coverage_line_uses_rounded_percent() {
        let line = format_coverage_counts(&SnapshotExtras {
            orphan: 0,
            covered: 1,
            coverable: 2,
        });
        assert_eq!(line, "Coverage: 1 covered, 2 coverable (50%)");
        assert_eq!(
            format_coverage_counts(&SnapshotExtras {
                orphan: 0,
                covered: 0,
                coverable: 0,
            }),
            "Coverage: 0 covered, 0 coverable (100%)"
        );
    }
}
