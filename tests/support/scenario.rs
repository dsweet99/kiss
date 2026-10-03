use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, git_command, init_git_repo};
use crate::support::watch_proc::{
    WatchProc, spawn_watch_logged, start_watch_logged, write_kissconfig_with_threshold,
};

pub struct Reply {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Reply {
    fn from_output(out: Output) -> Self {
        Self {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    pub fn summary(&self) -> &str {
        self.stdout
            .lines()
            .rev()
            .find(|line| line.starts_with("✓ ") || line.starts_with("✗ "))
            .unwrap_or("")
    }
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "code={:?}\n--- stdout\n{}--- stderr\n{}",
            self.code, self.stdout, self.stderr
        )
    }
}

fn kiss_command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.args(args)
        .current_dir(dir)
        .env("PYTHONDONTWRITEBYTECODE", "1");
    cmd
}

pub fn kiss(dir: &Path, args: &[&str]) -> Reply {
    Reply::from_output(kiss_command(dir, args).output().expect("run kiss"))
}

pub fn spawn_kiss(dir: &Path, args: &[&str]) -> Child {
    kiss_command(dir, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kiss")
}

pub fn spawn_kiss_in_own_group(dir: &Path, args: &[&str]) -> Child {
    use std::os::unix::process::CommandExt;
    kiss_command(dir, args)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kiss")
}

pub fn ctrl_c(child: &mut Child) {
    let _ = unsafe { libc::kill(-(child.id() as i32), libc::SIGINT) };
    let deadline = Instant::now() + Duration::from_secs(2);
    while matches!(child.try_wait(), Ok(None)) {
        assert!(
            Instant::now() < deadline,
            "interrupted kiss test must exit quickly"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn finish(child: Child) -> Reply {
    Reply::from_output(child.wait_with_output().expect("wait for kiss"))
}

pub fn skip_under_coverage() -> bool {
    std::env::var_os("LLVM_PROFILE_FILE").is_some()
}

pub struct Scenario {
    repo: tempfile::TempDir,
    side: tempfile::TempDir,
}

impl Scenario {
    pub fn new() -> Self {
        let repo = tempfile::TempDir::new().unwrap();
        init_git_repo(repo.path());
        Self {
            repo,
            side: tempfile::TempDir::new().unwrap(),
        }
    }

    pub fn root(&self) -> &Path {
        self.repo.path()
    }

    pub fn log(&self) -> PathBuf {
        self.side.path().join("watch.log")
    }

    pub fn marker(&self) -> PathBuf {
        self.side.path().join("runs.txt")
    }

    pub fn write(&self, rel: &str, contents: &str) {
        let path = self.root().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    pub fn replace_in(&self, rel: &str, from: &str, to: &str) -> String {
        let text = std::fs::read_to_string(self.root().join(rel)).unwrap();
        assert!(text.contains(from), "{rel} must contain {from:?}");
        text.replacen(from, to, 1)
    }

    pub fn commit(&self) {
        commit_all(self.root(), "init");
    }

    pub fn git(&self, args: &[&str]) {
        let status = git_command(self.root()).args(args).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    pub fn py_test(&self, name: &str, body: &str) -> String {
        format!(
            "def {name}():\n    with open({:?}, \"a\") as fh:\n        fh.write(\"{name}\\n\")\n    {body}\n\n\n",
            self.marker().to_str().unwrap()
        )
    }

    pub fn python_pass_fail(&self, settle: f64) {
        self.write(".gitignore", ".kiss/\n__pycache__/\ntarget/\n");
        self.write("lib_a.py", "def f():\n    return 0\n");
        self.write("lib_b.py", "def g():\n    return 1\n");
        let pass = self.py_test("test_pass", "assert f() == 0");
        self.write("test_a.py", &format!("from lib_a import f\n\n\n{pass}"));
        let fail = self.py_test("test_fail", "assert g() == 2");
        self.write("test_b.py", &format!("from lib_b import g\n\n\n{fail}"));
        write_kissconfig_with_threshold(self.root(), settle, 0);
    }

    pub fn python_with_slow(&self, settle: f64) {
        self.python_pass_fail(settle);
        let fast = self.py_test("test_pass", "assert f() == 0");
        let slow = self.py_test("test_slow", "time.sleep(int(f()) + 7)");
        self.write(
            "test_a.py",
            &format!("import time\n\nfrom lib_a import f\n\n\n{fast}{slow}"),
        );
    }

    pub fn rust_with_slow(&self) {
        self.rust_crate(
            "pub fn value() -> u32 {\n    1\n}\n",
            &[
                ("rs_pass", "assert_eq!(demo::value(), 1);"),
                ("rs_fail", "assert_eq!(demo::value(), 2);"),
                (
                    "rs_slow",
                    "std::thread::sleep(std::time::Duration::from_secs(u64::from(demo::value()) * 4));",
                ),
            ],
        );
    }

    pub fn rust_crate(&self, lib_body: &str, tests: &[(&str, &str)]) {
        self.write(
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        self.write("src/lib.rs", lib_body);
        let mut rs = format!(
            "fn mark(name: &str) {{\n    use std::io::Write;\n    let mut fh = std::fs::OpenOptions::new().create(true).append(true).open({:?}).unwrap();\n    writeln!(fh, \"{{name}}\").unwrap();\n}}\n",
            self.marker().to_str().unwrap()
        );
        for (name, body) in tests {
            rs.push_str(&format!(
                "\n#[test]\nfn {name}() {{\n    mark(\"{name}\");\n    {body}\n}}\n"
            ));
        }
        self.write("tests/it.rs", &rs);
        crate::common::generate_lockfile(self.root());
    }

    pub fn wait_for_run(&self) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while std::fs::read_to_string(self.marker())
            .unwrap_or_default()
            .is_empty()
        {
            assert!(Instant::now() < deadline, "no test ever started");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn take_runs(&self) -> Vec<String> {
        let marker = self.marker();
        let text = std::fs::read_to_string(&marker).unwrap_or_default();
        std::fs::write(&marker, "").unwrap();
        let mut runs: Vec<String> = text.lines().map(str::to_owned).collect();
        runs.sort();
        runs
    }

    pub fn wait_for_marker(&self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !std::fs::read_to_string(self.marker())
            .unwrap_or_default()
            .lines()
            .any(|line| line == name)
        {
            assert!(Instant::now() < deadline, "{name} never started");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn start_watch(&self) -> WatchProc {
        start_watch_logged(self.root(), &["test-watch"], &self.log())
    }

    pub fn spawn_watch(&self) -> WatchProc {
        spawn_watch_logged(self.root(), &["test-watch"], &self.log())
    }

    pub fn log_text(&self) -> String {
        std::fs::read_to_string(self.log()).unwrap_or_default()
    }

    pub fn starts(&self) -> usize {
        self.log_text().matches("kiss test: Starting").count()
    }

    pub fn requests(&self) -> usize {
        self.log_text().matches("kiss test: request ").count()
    }

    pub fn wait_log(&self, what: &str, done: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !done(&self.log_text()) {
            assert!(
                Instant::now() < deadline,
                "watcher log never showed {what}; log={}",
                self.log_text()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn wait_idle_after(&self, starts: usize) {
        self.wait_log("an idle watcher", |text| {
            text.matches("kiss test: Starting").count() >= starts
                && text.trim_end().ends_with("kiss test: Waiting")
        });
    }

    pub fn wait_settled(&self) {
        loop {
            self.wait_idle_after(1);
            let before = self.log_text();
            std::thread::sleep(Duration::from_millis(1500));
            if self.log_text() == before {
                return;
            }
        }
    }

    pub fn edit_until_testing(&self, rel: &str, contents: &str) {
        let starts = self.starts();
        self.write(rel, contents);
        self.wait_running_after(starts + 1);
        self.wait_log("the edit cycle running tests", |text| {
            text.rsplit("kiss test: Starting")
                .next()
                .is_some_and(|tail| tail.contains("tests_remaining="))
        });
    }

    pub fn wait_running_after(&self, starts: usize) {
        self.wait_log("a running cycle", |text| {
            text.matches("kiss test: Starting").count() >= starts
                && !text.trim_end().ends_with("kiss test: Waiting")
        });
    }
}

pub const WATCH_USAGE_ERRORS: [&[&str]; 13] = [
    &["--config", "ci.kissconfig"],
    &["-j", "4"],
    &["--metrics"],
    &["--coverage-all"],
    &["--ignore", "tests/unit/slow"],
    &["--retry-bad", "tests/unit"],
    &["--lang", "rust"],
    &["--dry-run"],
    &["--gobledygook"],
    &["tests/unit"],
    &["commit"],
    &["--lang", "rust", "tests/unit"],
    &["tests/unit", "--lang", "rust"],
];

pub fn assert_watch_usage_errors(dir: &Path) {
    for extra in WATCH_USAGE_ERRORS {
        let args: Vec<&str> = std::iter::once("test-watch")
            .chain(extra.iter().copied())
            .collect();
        let reply = kiss(dir, &args);
        assert_eq!(reply.code, Some(2), "{args:?}: {reply:?}");
        let expected = if extra[0].starts_with('-') {
            format!(
                "error: kiss test-watch: option is not accepted: {}",
                extra[0]
            )
        } else {
            format!(
                "error: kiss test-watch: TARGET is not accepted: {}",
                extra[0]
            )
        };
        assert!(
            reply.stderr.contains(&expected),
            "{args:?}: want {expected:?}; {reply:?}"
        );
        assert!(
            !reply.stderr.contains("already running"),
            "{args:?}: a usage error must not mention a running watcher; {reply:?}"
        );
    }
}

pub fn assert_repeats(reply: &Reply, needle: &str, phase: &str) {
    let waits = reply.stdout.matches(needle).count() + reply.stderr.matches(needle).count();
    assert!(
        waits >= 2,
        "{phase}: {needle:?} must repeat every 3 s; {reply:?}"
    );
}

pub fn assert_waits_for_watcher(reply: &Reply, phase: &str) {
    assert_repeats(reply, "kiss test: waiting for watcher (pid ", phase);
}
