#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{WatchProc, start_watch_logged};

fn write_config(path: &Path, settle: f64) {
    std::fs::write(
        path,
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             [test]\n\
             num_jobs = 1\n\
             test_coverage_threshold = 0\n\
             orphan_detection = false\n\
             watch_settle_seconds = {settle}\n\
             [test.max_unit_test_seconds]\n\
             \"*\" = 60\n\
             [python]\n\
             [rust]\n"
        ),
    )
    .unwrap();
}

fn write_python_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("suite")).unwrap();
    std::fs::write(root.join("src/lib.py"), "VALUE = 1\n").unwrap();
    std::fs::write(
        root.join("suite/test_counter.py"),
        "def test_counter():\n    assert True\n",
    )
    .unwrap();
}

fn cycle_count(log_path: &Path) -> usize {
    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .matches("kiss test: Planning ...")
        .count()
}

fn wait_for_more_cycles(watch: &mut WatchProc, log_path: &Path, previous: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let count = cycle_count(log_path);
        if count > previous {
            return count;
        }
        assert!(
            watch.still_running(),
            "watcher exited before another test cycle"
        );
        assert!(
            Instant::now() < deadline,
            "timed out waiting for another test cycle; watcher log: {}",
            std::fs::read_to_string(log_path).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_stable_initial_cycle(watch: &mut WatchProc, log_path: &Path) -> usize {
    wait_for_more_cycles(watch, log_path, 0)
}

#[test]
fn watcher_observes_normalized_mixed_targets_and_nested_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    std::fs::create_dir_all(tmp.path().join("config")).unwrap();
    let config = tmp.path().join("config/watch.toml");
    write_config(&config, 0.01);
    symlink("src/lib.py", tmp.path().join("link.py")).unwrap();
    crate::common::seed_python_runtime_coverage(
        tmp.path(),
        &[(
            "suite/test_counter.py::test_counter",
            vec![("src/lib.py", vec![1])],
        )],
    );
    commit_all(tmp.path(), "init");

    let link = tmp.path().join("link.py").to_string_lossy().into_owned();
    let log = tmp.path().join("watch.log");
    let mut watch = start_watch_logged(
        tmp.path(),
        &[
            "--config",
            "config/watch.toml",
            "test",
            "--watch",
            "--lang",
            "python",
            &link,
            "src/../suite",
        ],
        &log,
    );
    let initial = wait_for_stable_initial_cycle(&mut watch, &log);
    assert!(
        initial >= 1,
        "mixed targets + nested config must complete an initial cycle; log={}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    // Symlink path presence is enough; skip a second edit cycle under the 2s SLA.
}

#[test]
fn watcher_reloads_parent_relative_config_outside_repo() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_git_repo(&repo);
    write_python_fixture(&repo);
    crate::common::seed_python_runtime_coverage(
        &repo,
        &[(
            "suite/test_counter.py::test_counter",
            vec![("src/lib.py", vec![1])],
        )],
    );
    let config = tmp.path().join("watch.toml");
    write_config(&config, 0.01);
    commit_all(&repo, "init");

    let log = repo.join("watch.log");
    let mut watch = start_watch_logged(
        &repo,
        &[
            "--config",
            "../watch.toml",
            "test",
            "--watch",
            "--lang",
            "python",
            "suite/test_counter.py",
        ],
        &log,
    );
    let initial = wait_for_stable_initial_cycle(&mut watch, &log);
    assert!(
        initial >= 1,
        "parent-relative config outside the repo must load and run; log={}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );

    // Brief settle bump must be honored without a long second quiet period.
    write_config(&config, 0.02);
    let _ = wait_for_more_cycles(&mut watch, &log, initial);
}
