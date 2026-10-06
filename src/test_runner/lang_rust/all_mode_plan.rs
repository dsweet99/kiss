use std::path::Path;

use crate::test_runner::lang_iface::AllModePlan;

/// Plans every discovered Rust test. With no Rust records yet, the run is a full
/// population; otherwise the runtime runs only the tests whose records do not hold.
pub(super) fn rust_all_mode_plan(repo_root: &Path, mut selectors: Vec<String>) -> AllModePlan {
    selectors.sort();
    selectors.dedup();
    let population_required =
        !selectors.is_empty() && !kiss::test_records::records_dir(repo_root, "rust").is_dir();
    AllModePlan {
        planned: selectors,
        population_required,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn population_is_required_only_before_any_rust_record_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = rust_all_mode_plan(tmp.path(), vec!["b".into(), "a".into(), "a".into()]);
        assert_eq!(plan.planned, ["a", "b"]);
        assert!(plan.population_required);
        assert!(!rust_all_mode_plan(tmp.path(), Vec::new()).population_required);
        std::fs::create_dir_all(kiss::test_records::records_dir(tmp.path(), "rust")).unwrap();
        assert!(!rust_all_mode_plan(tmp.path(), vec!["a".into()]).population_required);
    }
}
