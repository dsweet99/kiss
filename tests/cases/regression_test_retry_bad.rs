//! Correctness of `kiss test --retry-bad` (report.md path 1).

use std::path::Path;
use std::process::Command;

use crate::support::git::{commit_all, init_git_repo};

fn write_retry_bad_fixture(dir: &Path) {
    std::fs::write(
        dir.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         test_coverage_threshold = 0\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    std::fs::write(dir.join("lib.py"), "VALUE = 0\n").unwrap();
    std::fs::write(
        dir.join("test_lib.py"),
        concat!(
            "import lib\n",
            "\n",
            "\n",
            "def test_ok():\n",
            "    assert True\n",
            "\n",
            "\n",
            "def test_flip():\n",
            "    assert lib.VALUE == 1\n",
        ),
    )
    .unwrap();
}

fn kiss_test(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    cmd.args(args)
        .current_dir(dir)
        .env("PYTHONPATH", dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("kiss test")
}

#[test]
fn retry_bad_reruns_only_prior_fail_after_fix() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_retry_bad_fixture(tmp.path());
    commit_all(tmp.path(), "init");

    let seed = kiss_test(tmp.path(), &["test", "--lang", "python", "."]);
    let seed_out = String::from_utf8_lossy(&seed.stdout);
    let seed_err = String::from_utf8_lossy(&seed.stderr);
    assert!(
        !seed.status.success(),
        "seed run must fail while VALUE is wrong; stdout={seed_out} stderr={seed_err}"
    );
    assert!(
        seed_out.contains("FAIL:") || seed_out.to_lowercase().contains("failed"),
        "seed run must mark FAIL; stdout={seed_out} stderr={seed_err}"
    );
    assert!(
        seed_out.contains("test_flip") || seed_out.contains("test_lib.py"),
        "seed FAIL must name the flip test; stdout={seed_out}"
    );

    std::fs::write(tmp.path().join("lib.py"), "VALUE = 1\n").unwrap();
    let retry = kiss_test(
        tmp.path(),
        &["test", "--lang", "python", ".", "--retry-bad"],
    );
    let retry_out = String::from_utf8_lossy(&retry.stdout);
    let retry_err = String::from_utf8_lossy(&retry.stderr);
    assert!(
        retry.status.success(),
        "retry-bad after fix must exit 0; stdout={retry_out} stderr={retry_err}"
    );
    assert!(
        retry_out.contains("PASS:") || retry_out.to_lowercase().contains("passed"),
        "retry-bad must report PASS; stdout={retry_out}"
    );
    // Prior PASS should be reused; only the former FAIL needs a live re-run.
    assert!(
        retry_out.contains("PASS (cached)") || retry_out.contains("(cached)"),
        "retry-bad must leave prior PASS cached; stdout={retry_out}"
    );
    assert!(
        retry_out.contains("test_flip"),
        "former FAIL must be reported after retry-bad; stdout={retry_out}"
    );
}
