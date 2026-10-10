use std::collections::BTreeMap;

pub(crate) fn report_strings_for_logical_strings(
    map: &BTreeMap<String, String>,
    logicals: &[String],
) -> Vec<String> {
    logicals
        .iter()
        .map(|logical| report_string_for_logical_string(map, logical))
        .collect()
}

pub(crate) fn report_string_for_logical_string(
    map: &BTreeMap<String, String>,
    logical: &str,
) -> String {
    map.get(logical)
        .cloned()
        .unwrap_or_else(|| logical.to_string())
}

pub(crate) fn qualified_rust_report_ids(
    repo_root: &std::path::Path,
    planned: &[String],
) -> BTreeMap<String, String> {
    if !planned.iter().any(|selector| selector.contains('$')) {
        return BTreeMap::new();
    }
    crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(repo_root, &[])
        .unwrap_or_default()
        .into_iter()
        .filter(|(logical, _)| logical.contains('$'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_logical_uses_the_report_id_and_unmapped_stays_itself() {
        let map = BTreeMap::from([("tests::case".into(), "src/lib.rs::case".into())]);
        assert_eq!(
            report_string_for_logical_string(&map, "tests::case"),
            "src/lib.rs::case"
        );
        assert_eq!(
            report_string_for_logical_string(&BTreeMap::new(), "bare_fn"),
            "bare_fn"
        );
    }

    #[test]
    fn report_strings_follow_the_same_lookup_for_each_logical() {
        let map = BTreeMap::from([
            ("tests::a".into(), "src/a.rs::a".into()),
            ("tests::b".into(), "src/b.rs::b".into()),
        ]);
        assert_eq!(
            report_strings_for_logical_strings(&map, &["tests::a".into(), "tests::b".into()]),
            vec!["src/a.rs::a".to_string(), "src/b.rs::b".to_string()]
        );
    }
}
