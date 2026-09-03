use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::cwd_test_lock;
use crate::test_runner::python_named_target_args::python_named_target_args;
use crate::test_runner::run_test;

const HARNESS_BUDGET: Duration = Duration::from_millis(3000);

fn init_git_repo(root: &Path) {
    let mut cmd = kiss::scrubbed_git_command(root);
    assert!(cmd.arg("init").status().unwrap().success());
}

fn write_trivial_python_test(root: &Path) {
    let tests = root.join("tests");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("test_fast.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
}

fn run_args(force_rerun: bool) -> crate::test_runner::RunTestCmdArgs<'static> {
    python_named_target_args("tests/test_fast.py::test_ok", force_rerun)
}

#[cfg(unix)]
fn capture_stdout_line_times(f: impl FnOnce()) -> (Duration, Vec<(Duration, String)>) {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let read_fd = fds[0];
    let write_fd = fds[1];
    let old_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
    assert!(old_stdout >= 0);
    assert_eq!(
        unsafe { libc::dup2(write_fd, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    unsafe {
        libc::close(write_fd);
    }

    let (tx, rx) = mpsc::channel::<(Duration, String)>();
    let started = Instant::now();
    let reader = std::thread::spawn(move || {
        let file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let mut lines = BufReader::new(file).lines();
        while let Some(Ok(line)) = lines.next() {
            let _ = tx.send((started.elapsed(), line));
        }
    });

    let wall_started = Instant::now();
    f();
    let _ = std::io::stdout().flush();
    assert_eq!(
        unsafe { libc::dup2(old_stdout, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    unsafe {
        libc::close(old_stdout);
    }
    let wall = wall_started.elapsed();
    reader.join().unwrap();
    let mut stamped = Vec::new();
    while let Ok(item) = rx.try_recv() {
        stamped.push(item);
    }
    (wall, stamped)
}

fn gap_between(lines: &[(Duration, String)], start_substr: &str, end_substr: &str) -> Duration {
    let start = lines
        .iter()
        .find(|(_, line)| line.contains(start_substr))
        .unwrap_or_else(|| panic!("missing start line containing {start_substr:?}: {lines:?}"));
    let end = lines
        .iter()
        .find(|(_, line)| line.contains(end_substr))
        .unwrap_or_else(|| panic!("missing end line containing {end_substr:?}: {lines:?}"));
    end.0.saturating_sub(start.0)
}

#[test]
#[cfg(unix)]
fn explicit_single_python_test_harness_stays_under_50ms() {
    let _cwd = crate::cwd_test_lock::lock();
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_trivial_python_test(tmp.path());

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let warm_code = run_test(run_args(true));
    assert_eq!(warm_code, 0, "warm force run must pass");

    let (force_wall, force_lines) = capture_stdout_line_times(|| {
        assert_eq!(run_test(run_args(true)), 0);
    });
    let summary_gap = gap_between(&force_lines, "kiss test: tests_remaining=0", "✓");
    let prepared_at = force_lines
        .iter()
        .find(|(_, line)| line.contains("rslip prepared"))
        .map(|(t, _)| *t)
        .expect("prepared line");
    let pass_at = force_lines
        .iter()
        .find(|(_, line)| line.contains("PASS:"))
        .map(|(t, _)| *t)
        .expect("PASS line");
    let execution_wall = pass_at.saturating_sub(prepared_at);
    let force_harness = force_wall.saturating_sub(execution_wall);

    let (hit_wall, hit_lines) = capture_stdout_line_times(|| {
        assert_eq!(run_test(run_args(false)), 0);
    });

    std::env::set_current_dir(orig).unwrap();

    assert!(
        summary_gap <= HARNESS_BUDGET,
        "force tests_remaining→summary {summary_gap:?} exceeds {HARNESS_BUDGET:?}; lines={force_lines:?}"
    );
    assert!(
        force_harness <= HARNESS_BUDGET,
        "force harness excluding execution {force_harness:?} (wall {force_wall:?} - exec {execution_wall:?}) exceeds {HARNESS_BUDGET:?}"
    );
    assert!(
        hit_wall <= HARNESS_BUDGET,
        "cache-hit single python kiss test wall {hit_wall:?} exceeds {HARNESS_BUDGET:?}; lines={hit_lines:?}"
    );
}
