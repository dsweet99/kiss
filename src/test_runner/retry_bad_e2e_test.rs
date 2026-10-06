//! In-process e2e: `kiss test --retry-bad` keeps prior PASS cached and reruns FAIL.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::bin_cli::args::TestInvocation;
use crate::cwd_test_lock;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::capture_stdout::capture_stdout;
use crate::test_runner::run_test;

fn init_git(root: &Path) {
    assert!(
        kiss::scrubbed_git_command(root)
            .arg("init")
            .status()
            .unwrap()
            .success()
    );
}

fn write_fixture(root: &Path, flag: &Path) {
    fs::write(
        root.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    fs::write(root.join("lib.py"), "VALUE = 1\n").unwrap();
    fs::write(
        root.join("test_lib.py"),
        format!(
            "import os\n\nimport lib\n\n\ndef test_ok():\n    assert True\n\n\n\
def test_flip():\n    assert lib.VALUE == 1\n    assert not os.path.exists({:?})\n",
            flag.to_string_lossy()
        ),
    )
    .unwrap();
}

fn test_args(force_bad: bool) -> RunTestCmdArgs<'static> {
    let gate = kiss::GateConfig {
        orphan_detection: false,
        max_unit_test_seconds: Vec::new(),
        ..Default::default()
    };
    RunTestCmdArgs {
        doubles: None,
        invocation: TestInvocation::Targets(vec![
            "test_lib.py::test_ok".into(),
            "test_lib.py::test_flip".into(),
        ]),
        target_request: crate::test_runner::target_request::operands_request(
            &[
                "test_lib.py::test_ok".into(),
                "test_lib.py::test_flip".into(),
            ],
            Some(kiss::Language::Python),
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
        force_bad,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: gate,
    }
}

fn run_in(root: &Path, force_bad: bool) -> (i32, String) {
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(root).unwrap();
    let mut code = 1;
    let out = capture_stdout(|| {
        code = run_test(test_args(force_bad));
    });
    std::env::set_current_dir(orig).unwrap();
    (code, out)
}

#[test]
fn retry_bad_keeps_prior_pass_cached_and_reruns_fail() {
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let flag = outside.path().join("flip.flag");
    fs::write(&flag, "").unwrap();
    init_git(tmp.path());
    write_fixture(tmp.path(), &flag);

    let (code, out) = run_in(tmp.path(), false);
    assert_eq!(
        code, 1,
        "first run must record test_flip as FAIL; out={out}"
    );
    assert!(
        out.contains("FAIL") && out.contains("test_flip"),
        "out={out}"
    );

    fs::remove_file(&flag).unwrap();
    let (code, out) = run_in(tmp.path(), true);
    assert_eq!(code, 0, "retry-bad must exit 0; out={out}");
    assert!(
        out.contains("misses=1") && !out.contains("PASS: test_lib.py::test_ok"),
        "prior PASS must stay cached; out={out}"
    );
    assert!(
        !out.contains("PASS test_lib.py::test_ok"),
        "a cached PASS has no line; out={out}"
    );
    assert!(
        out.contains("PASS:") && out.contains("test_flip"),
        "prior FAIL must rerun; out={out}"
    );
}
