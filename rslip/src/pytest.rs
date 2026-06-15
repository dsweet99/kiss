use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{PytestTraceCollector, TestCoverageRun};
use crate::util::content_digest;

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(0);

impl PytestTraceCollector {
    pub fn collect(
        &self,
        repo_root: &Path,
        selectors: &[String],
    ) -> Result<Vec<TestCoverageRun>, String> {
        if selectors.is_empty() {
            return Ok(Vec::new());
        }
        let (input, output) = trace_paths(selectors);
        write_selector_input(&input, selectors)?;
        let output_status = run_trace_process(repo_root, &input, &output)?;
        let _ = fs::remove_file(&input);
        if !output_status.status.success() {
            let _ = fs::remove_file(&output);
            return Err(format!(
                "coverage collection failed for pytest selector batch\n{}{}",
                String::from_utf8_lossy(&output_status.stdout),
                String::from_utf8_lossy(&output_status.stderr)
            ));
        }
        let raw = read_trace_output(&output)?;
        Ok(runs_from_raw(selectors, raw))
    }
}

fn trace_paths(selectors: &[String]) -> (PathBuf, PathBuf) {
    let token = content_digest(selectors.join("\n").as_bytes());
    let nonce = TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let input = std::env::temp_dir().join(format!(
        "kiss-rslip-{}-{nonce}-{token}.in.json",
        std::process::id()
    ));
    let output = std::env::temp_dir().join(format!(
        "kiss-rslip-{}-{nonce}-{token}.out.json",
        std::process::id()
    ));
    (input, output)
}

fn write_selector_input(input: &Path, selectors: &[String]) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(selectors).map_err(|e| format!("failed to encode selectors: {e}"))?;
    fs::write(input, bytes).map_err(|e| format!("failed to write {}: {e}", input.display()))
}

fn trace_script() -> &'static str {
    r#"
import json, os, sys
repo = os.path.realpath(sys.argv[1])
selectors_path = sys.argv[2]
out_path = sys.argv[3]
with open(selectors_path, encoding="utf-8") as fh:
    selectors = json.load(fh)
hits = {selector: {} for selector in selectors}
collection_hits = {}
current = None
canonical_filenames = {}
def tracer(frame, event, arg):
    if event == "line":
        raw_filename = frame.f_code.co_filename
        filename = canonical_filenames.get(raw_filename)
        if filename is None:
            filename = os.path.realpath(raw_filename)
            canonical_filenames[raw_filename] = filename
        if filename.startswith(repo + os.sep):
            rel = os.path.relpath(filename, repo).replace(os.sep, "/")
            target = collection_hits if current is None else hits.setdefault(current, {})
            target.setdefault(rel, set()).add(frame.f_lineno)
    return tracer
class RslipPlugin:
    def pytest_runtest_setup(self, item):
        global current
        current = item.nodeid
    def pytest_runtest_teardown(self, item, nextitem):
        global current
        current = None
try:
    import pytest
except Exception as exc:
    print(f"failed to import pytest: {exc}", file=sys.stderr)
    sys.exit(97)
sys.path.insert(0, repo)
sys.settrace(tracer)
try:
    code = pytest.main(selectors + ["-q"], plugins=[RslipPlugin()])
finally:
    sys.settrace(None)
serializable = {
    selector: {
        path: sorted(set(lines) | set(collection_hits.get(path, set())))
        for path, lines in (collection_hits | per_file).items()
    }
    for selector, per_file in hits.items()
}
with open(out_path, "w", encoding="utf-8") as fh:
    json.dump(serializable, fh)
sys.exit(code)
"#
}

fn run_trace_process(repo_root: &Path, input: &Path, output: &Path) -> Result<Output, String> {
    Command::new("python")
        .arg("-c")
        .arg(trace_script())
        .arg(repo_root)
        .arg(input)
        .arg(output)
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("failed to run pytest trace batch: {e}"))
}

fn read_trace_output(
    output: &Path,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<usize>>>, String> {
    let bytes = fs::read(output)
        .map_err(|e| format!("failed to read trace output {}: {e}", output.display()))?;
    let _ = fs::remove_file(output);
    serde_json::from_slice(&bytes).map_err(|e| format!("failed to parse trace output: {e}"))
}

fn runs_from_raw(
    selectors: &[String],
    raw: BTreeMap<String, BTreeMap<String, Vec<usize>>>,
) -> Vec<TestCoverageRun> {
    selectors
        .iter()
        .map(|selector| {
            let hits = raw_hits_for_selector(selector, &raw);
            let test_path = selector
                .split_once("::")
                .map_or_else(|| PathBuf::from(selector), |(path, _)| PathBuf::from(path));
            TestCoverageRun {
                selector: selector.clone(),
                test_path,
                hits,
            }
        })
        .collect()
}

fn raw_hits_for_selector(
    selector: &str,
    raw: &BTreeMap<String, BTreeMap<String, Vec<usize>>>,
) -> BTreeMap<PathBuf, BTreeSet<usize>> {
    let mut hits: BTreeMap<PathBuf, BTreeSet<usize>> = BTreeMap::new();
    for (nodeid, per_file) in raw {
        if !runtime_node_matches_selector(selector, nodeid) {
            continue;
        }
        for (path, lines) in per_file {
            hits.entry(PathBuf::from(path))
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    hits
}

fn runtime_node_matches_selector(selector: &str, nodeid: &str) -> bool {
    nodeid == selector
        || nodeid
            .strip_prefix(selector)
            .is_some_and(|suffix| suffix.starts_with('['))
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
    fn trace_helpers_round_trip_selectors_and_raw_hits() {
        let tmp = TempDir::new().unwrap();
        let selectors = vec!["test_sample.py::test_value".to_string()];
        let (input, output) = trace_paths(&selectors);
        assert_ne!(input, output);

        write_selector_input(&input, &selectors).unwrap();
        let encoded = fs::read_to_string(&input).unwrap();
        assert!(encoded.contains("test_sample.py::test_value"));
        let _ = fs::remove_file(&input);

        assert!(trace_script().contains("pytest.main"));
        fs::write(
            tmp.path().join("trace.json"),
            r#"{"test_sample.py::test_value":{"sample.py":[1,2]}}"#,
        )
        .unwrap();
        let raw = read_trace_output(&tmp.path().join("trace.json")).unwrap();
        let runs = runs_from_raw(&selectors, raw);
        assert_eq!(runs[0].test_path, PathBuf::from("test_sample.py"));
        assert_eq!(
            runs[0].hits[&PathBuf::from("sample.py")],
            [1, 2].into_iter().collect()
        );
    }

    #[test]
    fn parametrized_runtime_node_matching_is_selector_bounded() {
        assert!(runtime_node_matches_selector(
            "test_sample.py::test_value",
            "test_sample.py::test_value"
        ));
        assert!(runtime_node_matches_selector(
            "test_sample.py::test_value",
            "test_sample.py::test_value[case-with[brackets]]"
        ));
        assert!(!runtime_node_matches_selector(
            "test_sample.py::test_value",
            "test_sample.py::test_value_extra[case]"
        ));
        assert!(!runtime_node_matches_selector(
            "test_sample.py::test_value",
            "test_sample.py::test_value::nested"
        ));
    }

    #[test]
    fn run_trace_process_executes_the_trace_script() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("sample.py"),
            "def value():\n    return 3\n",
        );
        write(
            &tmp.path().join("test_sample.py"),
            "from sample import value\n\ndef test_value():\n    assert value() == 3\n",
        );
        let selectors = vec!["test_sample.py::test_value".to_string()];
        let input = tmp.path().join("selectors.json");
        let output = tmp.path().join("trace.json");
        write_selector_input(&input, &selectors).unwrap();

        let process = run_trace_process(tmp.path(), &input, &output).unwrap();

        assert!(process.status.success());
        let raw = read_trace_output(&output).unwrap();
        assert!(raw["test_sample.py::test_value"].contains_key("sample.py"));
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
            .collect(tmp.path(), &["test_sample.py::test_value".to_string()])
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
        write(
            &real_root.join("sample.py"),
            "def value():\n    return 3\n",
        );
        write(
            &real_root.join("test_sample.py"),
            "from sample import value\n\ndef test_value():\n    assert value() == 3\n",
        );

        let collector = PytestTraceCollector;
        let runs = collector
            .collect(&link_root, &["test_sample.py::test_value".to_string()])
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
    fn pytest_trace_collector_merges_parametrized_case_hits() {
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
        let runs = collector
            .collect(tmp.path(), &["test_sample.py::test_is_even".to_string()])
            .unwrap();

        assert_eq!(runs.len(), 1);
        let sample_hits = runs[0]
            .hits
            .get(&PathBuf::from("sample.py"))
            .expect("sample.py runtime hits");
        assert!(
            sample_hits.contains(&1) && sample_hits.contains(&2),
            "parametrized runtime cases should be merged into the discovered selector, got {sample_hits:?}"
        );
    }
}
