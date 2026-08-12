//! Distinct identities for Rust test selectors.
//!
//! Nextest / witnesses use logical ids (`tests::fn`, bare names). Gate patterns and
//! reporting expect `PATH::symbol` report ids. Mixing the two as bare `String` made
//! silent catch-all bans (`["*", 0]`) easy; these newtypes make the conversion
//! boundary explicit.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;

/// Nextest-facing selector id (bare fn name or `tests::fn`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LogicalSelectorId(String);

/// Kiss report / gate-pattern selector id (`path/to/file.rs::symbol`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ReportSelectorId(String);

impl LogicalSelectorId {
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[allow(dead_code)] // paired with ReportSelectorId::into_string at conversion boundaries
    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl ReportSelectorId {
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[allow(dead_code)] // used by unit tests; Deref covers most call sites
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl Deref for LogicalSelectorId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Deref for ReportSelectorId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for LogicalSelectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for ReportSelectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for LogicalSelectorId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for LogicalSelectorId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ReportSelectorId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ReportSelectorId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Convert a logical nextest id to a report id using the workspace map.
///
/// Falls back to the logical string when unmapped (caller must treat that as
/// report-shaped already, or accept catch-all gate behavior).
pub(crate) fn report_id_for_logical(
    map: &BTreeMap<String, String>,
    logical: &LogicalSelectorId,
) -> ReportSelectorId {
    ReportSelectorId::new(crate::test_runner::runners::kiss_test_report_id(
        map,
        logical.as_str(),
    ))
}

/// Map logical selector strings to report-id strings via the typed boundary.
///
/// Call sites that still hold `String` slices convert here so logical vs report
/// identities cannot be mixed without an explicit `LogicalSelectorId` step.
pub(crate) fn report_strings_for_logical_strings(
    map: &BTreeMap<String, String>,
    logicals: &[String],
) -> Vec<String> {
    let typed: Vec<LogicalSelectorId> = logicals.iter().cloned().map(LogicalSelectorId::new).collect();
    report_ids_for_logicals(map, &typed)
        .into_iter()
        .map(ReportSelectorId::into_string)
        .collect()
}

/// Convert one logical string through the typed boundary.
pub(crate) fn report_string_for_logical_string(
    map: &BTreeMap<String, String>,
    logical: &str,
) -> String {
    report_id_for_logical(map, &LogicalSelectorId::new(logical)).into_string()
}

/// Map a slice of logical ids to report ids (same order).
pub(crate) fn report_ids_for_logicals(
    map: &BTreeMap<String, String>,
    logicals: &[LogicalSelectorId],
) -> Vec<ReportSelectorId> {
    logicals
        .iter()
        .map(|logical| report_id_for_logical(map, logical))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_and_report_ids_are_distinct_types() {
        let logical = LogicalSelectorId::new("tests::case");
        let report = ReportSelectorId::new("src/lib.rs::case");
        assert_eq!(logical.as_str(), "tests::case");
        assert_eq!(report.as_str(), "src/lib.rs::case");
        assert_eq!(logical.clone().into_string(), "tests::case");
        // Conversion requires an explicit map — no silent String coercion either way.
        let map = BTreeMap::from([("tests::case".into(), "src/lib.rs::case".into())]);
        assert_eq!(
            report_id_for_logical(&map, &logical).as_str(),
            "src/lib.rs::case"
        );
    }

    #[test]
    fn selector_id_display_deref_and_from_cover_both_types() {
        let logical: LogicalSelectorId = "bare".into();
        let logical_owned: LogicalSelectorId = String::from("owned").into();
        let report: ReportSelectorId = "src/a.rs::t".into();
        let report_owned: ReportSelectorId = String::from("src/b.rs::t").into();
        assert_eq!(&*logical, "bare");
        assert_eq!(&*logical_owned, "owned");
        assert_eq!(&*report, "src/a.rs::t");
        assert_eq!(&*report_owned, "src/b.rs::t");
        assert_eq!(format!("{logical}"), "bare");
        assert_eq!(format!("{report}"), "src/a.rs::t");
        assert_eq!(report_owned.into_string(), "src/b.rs::t");
    }

    #[test]
    fn unmapped_logical_stays_explicit_at_boundary() {
        let logical = LogicalSelectorId::new("bare_fn");
        let map = BTreeMap::new();
        let report = report_id_for_logical(&map, &logical);
        assert_eq!(
            report.as_str(),
            "bare_fn",
            "unmapped logical remains visible so gate code can fail loudly"
        );
    }

    #[test]
    fn report_strings_for_logical_strings_uses_typed_boundary() {
        let map = BTreeMap::from([
            ("tests::a".into(), "src/a.rs::a".into()),
            ("tests::b".into(), "src/b.rs::b".into()),
        ]);
        let out = report_strings_for_logical_strings(
            &map,
            &["tests::a".into(), "tests::b".into(), "unmapped".into()],
        );
        assert_eq!(
            out,
            vec![
                "src/a.rs::a".to_string(),
                "src/b.rs::b".to_string(),
                "unmapped".to_string()
            ]
        );
        assert_eq!(
            report_string_for_logical_string(&map, "tests::a"),
            "src/a.rs::a"
        );
        let typed = report_ids_for_logicals(
            &map,
            &[LogicalSelectorId::new("tests::a"), LogicalSelectorId::new("tests::b")],
        );
        assert_eq!(typed[0].as_str(), "src/a.rs::a");
        assert_eq!(typed[1].as_str(), "src/b.rs::b");
    }
}
