mod collect;
mod flags;
mod pool;
mod scripts;

pub use collect::collect_nodeids;
pub use flags::validate_pytest_extra;
pub use pool::{build_fork_argv, default_parallelism, run_pool, shell_quote_line, trace_pool};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn collect_nodeids_finds_parametrized_instances() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("test_param.py"),
            concat!(
                "import pytest\n",
                "@pytest.mark.parametrize('x', [1, 2])\n",
                "def test_values(x):\n",
                "    assert x > 0\n",
            ),
        );
        let nodeids = collect_nodeids(tmp.path(), &[]).unwrap();
        assert!(nodeids.iter().any(|id| id.contains("test_values[1]")));
        assert!(nodeids.iter().any(|id| id.contains("test_values[2]")));
    }

    #[test]
    fn collect_nodeids_honors_pyproject_testpaths() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("pkg");
        fs::create_dir_all(pkg.join("tests")).unwrap();
        write(
            &pkg.join("pyproject.toml"),
            "[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n",
        );
        write(
            &pkg.join("tests/test_pkg.py"),
            "def test_in_custom_path():\n    assert True\n",
        );
        let nodeids = collect_nodeids(&pkg, &[]).unwrap();
        assert_eq!(nodeids, vec!["tests/test_pkg.py::test_in_custom_path"]);
    }

    #[test]
    fn collect_errors_before_run() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("conftest.py"),
            "raise RuntimeError('broken conftest')\n",
        );
        write(&tmp.path().join("test_x.py"), "def test_x():\n    pass\n");
        let err = collect_nodeids(tmp.path(), &[]).unwrap_err();
        assert!(err.contains("collection failed"), "{err}");
    }

    #[test]
    fn fork_runs_exactly_one_nodeid() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("test_one.py"),
            "def test_ok():\n    assert True\n",
        );
        #[cfg(unix)]
        pool::fork_runs_exactly_one_nodeid(tmp.path(), "test_one.py::test_ok").unwrap();
    }

    #[test]
    fn scheduler_keeps_j_in_flight() {
        let nodeids: Vec<String> = (0..4).map(|i| format!("fake::node{i}")).collect();
        #[cfg(unix)]
        {
            let peak = pool::scheduler_peak_concurrency(&nodeids, 2, 0.3).unwrap();
            assert_eq!(peak, 2, "peak concurrency should equal J=2");
        }
    }

    #[test]
    fn scheduler_aggregates_failures() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("test_pass.py"),
            "def test_pass():\n    assert True\n",
        );
        write(
            &tmp.path().join("test_fail.py"),
            "def test_fail():\n    assert False\n",
        );
        let nodeids = vec![
            "test_pass.py::test_pass".to_string(),
            "test_fail.py::test_fail".to_string(),
        ];
        let code = run_pool(tmp.path(), &nodeids, 2, &[]).unwrap();
        assert_ne!(code, 0);
    }

    #[test]
    fn trace_hook_emits_line_hits_for_nodeid() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("sample.py"),
            "def value():\n    return 3\n",
        );
        write(
            &tmp.path().join("test_sample.py"),
            "from sample import value\n\ndef test_value():\n    assert value() == 3\n",
        );
        let nodeids = vec!["test_sample.py::test_value".to_string()];
        let (code, trace_dir) = trace_pool(tmp.path(), &nodeids, 1).unwrap();
        assert_eq!(code, 0);
        let entries: Vec<_> = fs::read_dir(&trace_dir).unwrap().collect();
        assert!(!entries.is_empty());
        let mut found = false;
        for entry in entries {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            if text.contains("sample.py") {
                found = true;
            }
        }
        assert!(found, "trace output should include sample.py hits");
    }

    #[test]
    fn isolation_module_global_reset_between_forks() {
        let tmp = TempDir::new().unwrap();
        write(
            &tmp.path().join("test_state.py"),
            concat!(
                "STATE = []\n",
                "def test_a():\n",
                "    STATE.append('a')\n",
                "    assert STATE == ['a']\n",
                "def test_b():\n",
                "    assert STATE == [], f'leaked state: {STATE}'\n",
            ),
        );
        let nodeids = vec![
            "test_state.py::test_a".to_string(),
            "test_state.py::test_b".to_string(),
        ];
        let code = run_pool(tmp.path(), &nodeids, 1, &[]).unwrap();
        assert_eq!(code, 0, "module global state must not leak across forks");
    }

    #[test]
    fn build_fork_argv_and_shell_quote_line() {
        let argv = build_fork_argv(Path::new("/repo"), "t.py::test_a", &["--tb=short".into()]);
        assert!(argv.iter().any(|s| s.contains("t.py::test_a")));
        let quoted =
            shell_quote_line(&["python".into(), "-m".into(), "pytest".into(), "a b".into()]);
        assert!(quoted.contains('\''));
    }

    #[test]
    fn default_parallelism_is_positive() {
        assert!(default_parallelism() >= 1);
    }
}
