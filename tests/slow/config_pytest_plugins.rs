use crate::support::scenario::{Scenario, kiss, skip_under_kiss_test};

fn plugin_config(plugins: &str) -> String {
    format!(
        "[global]\n\
         duplication_enabled = false\n\
         \n\
         [test]\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         pytest_plugins = {plugins}\n\
         \n\
         [test.max_unit_test_seconds]\n\
         \"*\" = 60\n\
         [python]\n\
         [rust]\n"
    )
}

#[test]
fn pytest_plugins_parametrize_runs_each_case() {
    if skip_under_kiss_test() {
        return;
    }
    let s = Scenario::new();
    s.write(".gitignore", ".kiss/\n__pycache__/\n");
    s.write(
        "kissplug.py",
        "def pytest_generate_tests(metafunc):\n    if \"n\" in metafunc.fixturenames:\n        metafunc.parametrize(\"n\", [1, 2])\n",
    );
    s.write("test_ok.py", "def test_ok(n):\n    assert n in (1, 2)\n");
    s.write(".kissconfig", &plugin_config("[\"kissplug\"]"));
    s.commit();

    let reply = kiss(s.root(), &["test", "--lang", "python"]);
    let text = format!("{reply:?}");
    assert!(
        reply.stdout.contains("PASS: test_ok.py::test_ok[1]"),
        "plugin-generated case [1] must run; {text}"
    );
    assert!(
        reply.stdout.contains("PASS: test_ok.py::test_ok[2]"),
        "plugin-generated case [2] must run; {text}"
    );
    assert_eq!(reply.code, Some(0), "{text}");
}

#[test]
fn pytest_plugins_missing_reports_collection_failure() {
    if skip_under_kiss_test() {
        return;
    }
    let s = Scenario::new();
    s.write(".gitignore", ".kiss/\n__pycache__/\n");
    s.write("test_ok.py", "def test_ok():\n    assert True\n");
    s.write(".kissconfig", &plugin_config("[\"no_such_plugin\"]"));
    s.commit();

    let reply = kiss(s.root(), &["test", "--lang", "python"]);
    let text = format!("{reply:?}");
    assert!(
        text.contains("no_such_plugin"),
        "a missing pytest plugin must name itself; {text}"
    );
    assert!(
        text.contains("pytest collection failed"),
        "a missing pytest plugin must surface the collection error; {text}"
    );
    assert_ne!(reply.code, Some(0), "{text}");
}
