use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{PytestTraceCollector, TestCoverageRun};

impl PytestTraceCollector {
    pub fn collect(
        &self,
        repo_root: &Path,
        nodeids: &[String],
        j: usize,
    ) -> Result<Vec<TestCoverageRun>, String> {
        if nodeids.is_empty() {
            return Ok(Vec::new());
        }
        let (code, trace_dir) = pyfork::trace_pool(repo_root, nodeids, j)?;
        if code != 0 {
            return Err(format!(
                "coverage collection failed for pytest nodeid pool (exit {code})"
            ));
        }
        let mut raw: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
        for entry in fs::read_dir(&trace_dir).map_err(|e| format!("read trace dir: {e}"))? {
            let entry = entry.map_err(|e| format!("read trace entry: {e}"))?;
            let bytes = fs::read(entry.path()).map_err(|e| format!("read trace file: {e}"))?;
            let chunk: BTreeMap<String, BTreeMap<String, Vec<usize>>> =
                serde_json::from_slice(&bytes).map_err(|e| format!("parse trace output: {e}"))?;
            for (nodeid, per_file) in chunk {
                raw.entry(nodeid).or_default().extend(per_file);
            }
        }
        let _ = fs::remove_dir_all(&trace_dir);
        Ok(runs_from_raw(nodeids, raw))
    }
}

fn runs_from_raw(
    nodeids: &[String],
    raw: BTreeMap<String, BTreeMap<String, Vec<usize>>>,
) -> Vec<TestCoverageRun> {
    nodeids
        .iter()
        .map(|nodeid| {
            let hits = raw_hits_for_nodeid(nodeid, &raw);
            let test_path = nodeid
                .split_once("::")
                .map_or_else(|| PathBuf::from(nodeid), |(path, _)| PathBuf::from(path));
            TestCoverageRun {
                selector: nodeid.clone(),
                test_path,
                hits,
            }
        })
        .collect()
}

fn raw_hits_for_nodeid(
    nodeid: &str,
    raw: &BTreeMap<String, BTreeMap<String, Vec<usize>>>,
) -> BTreeMap<PathBuf, BTreeSet<usize>> {
    let mut hits: BTreeMap<PathBuf, BTreeSet<usize>> = BTreeMap::new();
    if let Some(per_file) = raw.get(nodeid) {
        for (path, lines) in per_file {
            hits.entry(PathBuf::from(path))
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn trace_helpers_round_trip_nodeids_and_raw_hits() {
        let tmp = TempDir::new().unwrap();
        let nodeids = vec!["test_sample.py::test_value".to_string()];
        fs::write(
            tmp.path().join("trace.json"),
            r#"{"test_sample.py::test_value":{"sample.py":[1,2]}}"#,
        )
        .unwrap();
        let bytes = fs::read(tmp.path().join("trace.json")).unwrap();
        let raw: BTreeMap<String, BTreeMap<String, Vec<usize>>> =
            serde_json::from_slice(&bytes).unwrap();
        let runs = runs_from_raw(&nodeids, raw);
        assert_eq!(runs[0].test_path, PathBuf::from("test_sample.py"));
        assert_eq!(
            runs[0].hits[&PathBuf::from("sample.py")],
            [1, 2].into_iter().collect()
        );
    }

    #[test]
    fn pytest_trace_collector_records_runtime_line_hits() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("sample.py"),
            "def value():\n    return 3\n",
        );
        write(
            &tmp.path().join("test_sample.py"),
            "from sample import value\n\ndef test_value():\n    assert value() == 3\n",
        );

        let collector = PytestTraceCollector;
        let runs = collector
            .collect(tmp.path(), &["test_sample.py::test_value".to_string()], 1)
            .unwrap();

        assert_eq!(runs.len(), 1);
        let sample_hits = runs[0]
            .hits
            .get(&PathBuf::from("sample.py"))
            .expect("sample.py runtime hits");
        assert!(
            sample_hits.contains(&1) && sample_hits.contains(&2),
            "function definition and return line should be traced, got {sample_hits:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn pytest_trace_collector_records_hits_from_symlinked_repo_root() {
        let tmp = TempDir::new().unwrap();
        let real_root = tmp.path().join("real");
        let link_root = tmp.path().join("link");
        fs::create_dir(&real_root).unwrap();
        std::os::unix::fs::symlink(&real_root, &link_root).unwrap();
        write(&real_root.join("sample.py"), "def value():\n    return 3\n");
        write(
            &real_root.join("test_sample.py"),
            "from sample import value\n\ndef test_value():\n    assert value() == 3\n",
        );

        let collector = PytestTraceCollector;
        let runs = collector
            .collect(&link_root, &["test_sample.py::test_value".to_string()], 1)
            .unwrap();

        assert_eq!(runs.len(), 1);
        let sample_hits = runs[0]
            .hits
            .get(&PathBuf::from("sample.py"))
            .expect("sample.py runtime hits from symlinked repo root");
        assert!(
            sample_hits.contains(&1) && sample_hits.contains(&2),
            "symlinked repo roots should record canonical file hits under relative paths, got {sample_hits:?}"
        );
    }

    #[test]
    fn pytest_trace_collector_stores_parametrized_nodeids_separately() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("sample.py"),
            "def is_even(value):\n    return value % 2 == 0\n",
        );
        write(
            &tmp.path().join("test_sample.py"),
            concat!(
                "import pytest\n",
                "from sample import is_even\n\n",
                "@pytest.mark.parametrize('value', [2, 4])\n",
                "def test_is_even(value):\n",
                "    assert is_even(value)\n",
            ),
        );

        let collector = PytestTraceCollector;
        let nodeids = pyfork::collect_nodeids(tmp.path(), &[]).unwrap();
        let runs = collector.collect(tmp.path(), &nodeids, 2).unwrap();

        assert_eq!(runs.len(), 2);
        for run in &runs {
            let sample_hits = run
                .hits
                .get(&PathBuf::from("sample.py"))
                .expect("sample.py runtime hits");
            assert!(
                sample_hits.contains(&1) && sample_hits.contains(&2),
                "parametrized nodeids should each have hits, got {sample_hits:?}"
            );
        }
    }
}
