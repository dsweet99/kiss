#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{WatchProc, start_watch_logged, wait_watch_idle_cycle};

fn write_config(path: &Path, settle: f64) {
    std::fs::write(
        path,
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             [test]\n\
             num_jobs = 1\n\
             test_coverage_threshold = 0\n\
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
    let _ = wait_for_more_cycles(watch, log_path, 0);
    wait_watch_idle_cycle(log_path.parent().unwrap());
    cycle_count(log_path)
}

#[test]
fn watcher_observes_normalized_mixed_targets_and_nested_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_fixture(tmp.path());
    std::fs::create_dir_all(tmp.path().join("config")).unwrap();
    let config = tmp.path().join("config/watch.toml");
    write_config(&config, 0.1);
    symlink("src/lib.py", tmp.path().join("link.py")).unwrap();
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

    std::fs::write(tmp.path().join("src/lib.py"), "VALUE = 2\n").unwrap();
    let after_file = wait_for_more_cycles(&mut watch, &log, initial);
    wait_watch_idle_cycle(tmp.path());

    std::fs::write(
        tmp.path().join("suite/test_counter.py"),
        "def test_counter():\n    assert 1 == 1\n",
    )
    .unwrap();
    let after_dir = wait_for_more_cycles(&mut watch, &log, after_file);
    wait_watch_idle_cycle(tmp.path());

    write_config(&config, 0.2);
    let _ = wait_for_more_cycles(&mut watch, &log, after_dir);
}

#[test]
fn watcher_reloads_parent_relative_config_outside_repo() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_git_repo(&repo);
    write_python_fixture(&repo);
    let config = tmp.path().join("watch.toml");
    write_config(&config, 0.1);
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

    write_config(&config, 0.2);
    let _ = wait_for_more_cycles(&mut watch, &log, initial);
}
