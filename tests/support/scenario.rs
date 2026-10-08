use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

pub fn write_kissconfig(root: &Path) {
    std::fs::write(
        root.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         \n\
         [test]\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         \n\
         [test.max_unit_test_seconds]\n\
         \"*\" = 60\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
}

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
        let line = self
            .stdout
            .lines()
            .rev()
            .find(|line| line.starts_with("✓ ") || line.starts_with("✗ "))
            .unwrap_or("");
        count_prefix(line)
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

fn count_prefix(line: &str) -> &str {
    let mut separators = 0;
    for (index, _) in line.match_indices(" · ") {
        separators += 1;
        if separators == 3 {
            return &line[..index];
        }
    }
    line
}

fn kiss_command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_build_env(&mut cmd);
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

pub fn skip_under_kiss_test() -> bool {
    std::env::var("NEXTEST_PROFILE").is_ok_and(|profile| profile == "kiss")
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

    pub fn py_test(&self, name: &str, body: &str) -> String {
        format!(
            "def {name}():\n    with open({:?}, \"a\") as fh:\n        fh.write(\"{name}\\n\")\n    {body}\n\n\n",
            self.marker().to_str().unwrap()
        )
    }

    pub fn python_pass_fail(&self) {
        self.write(".gitignore", ".kiss/\n__pycache__/\ntarget/\n");
        self.write("lib_a.py", "def f():\n    return 0\n");
        self.write("lib_b.py", "def g():\n    return 1\n");
        let pass = self.py_test("test_pass", "assert f() == 0");
        self.write("test_a.py", &format!("from lib_a import f\n\n\n{pass}"));
        let fail = self.py_test("test_fail", "assert g() == 2");
        self.write("test_b.py", &format!("from lib_b import g\n\n\n{fail}"));
        write_kissconfig(self.root());
    }

    pub fn python_with_slow(&self) {
        self.python_pass_fail();
        let fast = self.py_test("test_pass", "assert f() == 0");
        let slow = self.py_test("test_slow", "time.sleep(int(f()) + 7)");
        self.write(
            "test_a.py",
            &format!("import time\n\nfrom lib_a import f\n\n\n{fast}{slow}"),
        );
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
}

pub fn assert_repeats(reply: &Reply, needle: &str, phase: &str) {
    let waits = reply.stdout.matches(needle).count() + reply.stderr.matches(needle).count();
    assert!(
        waits >= 2,
        "{phase}: {needle:?} must repeat every 3 s; {reply:?}"
    );
}
