#![allow(dead_code)]

use fs2::FileExt;
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use tree_sitter::Node;

pub fn is_cli_wall_timing_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("kiss: ") else {
        return false;
    };
    rest.ends_with("ms") || (rest.ends_with('s') && rest.contains('.'))
}

pub fn without_cli_wall_timing(s: &str) -> String {
    s.lines()
        .filter(|line| !is_cli_wall_timing_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prefer tmpfs for TempDir-backed publish barriers (ext4 /tmp fsync is slow).
pub fn prefer_tmpfs_tmpdir() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("TMPDIR").is_none() && Path::new("/dev/shm").is_dir() {
            unsafe { std::env::set_var("TMPDIR", "/dev/shm") };
        }
    });
}

fn force_tmpfs_on_load() {
    prefer_tmpfs_tmpdir();
}

/// Touch prefer_tmpfs as soon as common is first used.
static FORCE_TMPFS: Once = Once::new();
fn ensure_tmpfs() {
    FORCE_TMPFS.call_once(force_tmpfs_on_load);
}

pub fn cache_dir_under(repo: &Path) -> PathBuf {
    repo.join(".kiss")
}

pub const BUILTIN_LANGUAGE_CONFIG: &str = "[python]\n[rust]\n";

pub fn write_builtin_language_config(dir: &Path) -> PathBuf {
    let path = dir.join("builtin.kissconfig");
    fs::write(&path, BUILTIN_LANGUAGE_CONFIG).unwrap();
    path
}

pub fn is_full_check_cache_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("check_full_")
        && Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
}

pub fn list_full_check_cache_files(repo: &Path) -> Vec<PathBuf> {
    let dir = cache_dir_under(repo);
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| is_full_check_cache_file(p))
        .collect();
    out.sort();
    out
}

pub fn generate_lockfile(repo: &Path) {
    let lockfile = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(repo)
        .output()
        .expect("cargo generate-lockfile should run");
    assert!(
        lockfile.status.success(),
        "cargo generate-lockfile failed: {}",
        String::from_utf8_lossy(&lockfile.stderr)
    );
}

/// Strip parent build and profiling env so nested cargo work is not inflated under suite load.
pub fn scrub_parent_build_env(cmd: &mut Command) {
    const KEYS: &[&str] = &[
        "LLVM_PROFILE_FILE",
        "LLVM_PROFILE_FILE_NAME",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_TARGET_DIR",
    ];
    for key in KEYS {
        cmd.env_remove(key);
    }
    // Cap nested cargo parallelism so suite-load contention is less likely to breach the 60s SLA,
    // while still allowing a little parallelism so fixture compiles finish under 2s.
    cmd.env("CARGO_BUILD_JOBS", "2");
}

/// Keep rustup/cargo usable when a test overrides `HOME` to a TempDir.
pub fn preserve_toolchain_homes(cmd: &mut Command) {
    if std::env::var_os("RUSTUP_HOME").is_none()
        && let Some(home) = std::env::var_os("HOME")
    {
        cmd.env("RUSTUP_HOME", PathBuf::from(home).join(".rustup"));
    }
    if std::env::var_os("CARGO_HOME").is_none()
        && let Some(home) = std::env::var_os("HOME")
    {
        cmd.env("CARGO_HOME", PathBuf::from(home).join(".cargo"));
    }
}

pub fn persistent_cargo_target(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("kiss-test-targets").join(name);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn persistent_rust_failure_repo() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-rust-failure-fixture");
        if root.join("Cargo.toml").is_file() {
            return root;
        }
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join(".kissconfig"),
            "[global]\nduplication_enabled = false\n[test]\norphan_detection = false\n[python]\n[rust]\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("lib.rs"),
            "pub fn value() -> u32 { 1 }\n\
#[cfg(test)]\n\
mod tests {\n\
    #[test]\n\
    fn gets_value() {\n\
        assert_eq!(super::value(), 2);\n\
    }\n\
}\n",
        )
        .unwrap();
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "kiss")
            .env("GIT_AUTHOR_EMAIL", "kiss@test")
            .env("GIT_COMMITTER_NAME", "kiss")
            .env("GIT_COMMITTER_EMAIL", "kiss@test")
            .output()
            .expect("git commit files");
        assert!(commit.status.success(), "git commit files failed");
        let target_dir = persistent_cargo_target("rust-failure-demo");
        let status = Command::new("cargo")
            .args(["test", "--no-run", "-q"])
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .status()
            .expect("prebuild rust failure fixture");
        assert!(status.success(), "rust failure fixture prebuild failed");
        root
    })
    .clone()
}

pub fn persistent_python_failure_repo() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        // Unique per process so parallel nextest workers cannot share/mutate one fixture.
        let root = std::env::temp_dir().join(format!(
            "kiss-python-failure-fixture-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".kissconfig"),
            "[global]\nduplication_enabled = false\n[test]\norphan_detection = false\n[python]\n[rust]\n",
        )
        .unwrap();
        fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
        fs::write(
            root.join("test_lib.py"),
            "from lib import f\n\ndef test_f():\n    assert f() == 1\n",
        )
        .unwrap();
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "kiss")
            .env("GIT_AUTHOR_EMAIL", "kiss@test")
            .env("GIT_COMMITTER_NAME", "kiss")
            .env("GIT_COMMITTER_EMAIL", "kiss@test")
            .output()
            .expect("git commit files");
        assert!(commit.status.success(), "git commit files failed");
        root
    })
    .clone()
}

pub fn copy_repo_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("copy destination");
    let status = Command::new("cp")
        .args([
            "-a",
            &format!("{}/.", src.display()),
            &format!("{}/", dst.display()),
        ])
        .status()
        .expect("copy_repo_tree");
    assert!(status.success(), "copy_repo_tree failed");
    retarget_cloned_kiss_cache(src, dst);
}

fn retarget_cloned_kiss_cache(src: &Path, dst: &Path) {
    let old_root = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
    let new_root = dst.canonicalize().unwrap_or_else(|_| dst.to_path_buf());
    if old_root == new_root {
        return;
    }
    let old = old_root.to_string_lossy();
    let new = new_root.to_string_lossy();
    let kiss = new_root.join(".kiss");
    if !kiss.is_dir() {
        return;
    }
    let mut stack = vec![kiss];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_text = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json") || ext == "toml");
            if !is_text {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains(old.as_ref()) {
                continue;
            }
            fs::write(&path, text.replace(old.as_ref(), new.as_ref())).expect("retarget cache");
        }
    }
}

fn write_seeded_python_sources(root: &Path) {
    fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
    fs::write(
        root.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
}

/// Cross-process lock for the shared seeded-python fixture.
/// nextest workers are separate OS processes; an in-process Mutex does not
/// serialize rewrite of the persistent tree against concurrent `cp -a` clones.
/// Uses an exclusive flock (same class as production kiss-plan stores / force-python
/// fixture). A mkdir lock with timed steal can interrupt a live holder still
/// cloning under nextest -j12 load.
struct SeededPythonFixtureLock {
    _file: fs::File,
}

fn seeded_python_fixture_lock() -> SeededPythonFixtureLock {
    ensure_tmpfs();
    let path = std::env::temp_dir().join("kiss-seeded-python-fixture-v5.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .unwrap_or_else(|err| panic!("seeded python fixture lock {}: {err}", path.display()));
    file.lock_exclusive().unwrap_or_else(|err| {
        panic!(
            "seeded python fixture lock_exclusive {}: {err}",
            path.display()
        )
    });
    SeededPythonFixtureLock { _file: file }
}

/// Caller must hold `seeded_python_fixture_lock`.
fn persistent_seeded_python_repo_unlocked() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        // Ship a .gitignore so `.kiss/` / `target/` are never tracked; tracked
        // state dirt would flap commit stamps.
        let root = std::env::temp_dir().join("kiss-seeded-python-fixture-v5");
        if root.join(".git").join("HEAD").is_file() {
            // Reset sources under the flock so concurrent clones cannot tear a
            // mid-write lib.py / test_lib.py pair.
            write_seeded_python_sources(&root);
            return root;
        }
        fs::create_dir_all(&root).expect("seeded python fixture root");
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q", "-b", "main"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            kiss::scrubbed_git_command(&root)
                .args(["config", kv.0, kv.1])
                .status()
                .expect("git config");
        }
        fs::write(
            root.join(".gitignore"),
            "target/\n.kiss/\n__pycache__/\n*.pyc\n",
        )
        .unwrap();
        write_seeded_python_sources(&root);
        fs::write(
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
        let mut warm = Command::new(env!("CARGO_BIN_EXE_kiss"));
        scrub_parent_build_env(&mut warm);
        preserve_toolchain_homes(&mut warm);
        warm.env("PYTHONDONTWRITEBYTECODE", "1");
        let status = warm
            .args(["test", "--lang", "python", "."])
            .current_dir(&root)
            .status()
            .expect("warm seeded python fixture");
        assert!(status.success(), "seeded python fixture warm failed");
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "-A"])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .output()
            .expect("git commit");
        assert!(commit.status.success(), "git commit failed");
        root
    })
    .clone()
}

pub fn persistent_seeded_python_repo() -> PathBuf {
    let _lock = seeded_python_fixture_lock();
    persistent_seeded_python_repo_unlocked()
}

pub fn fresh_seeded_python_repo() -> tempfile::TempDir {
    // Hold the lock across clone so peers cannot rewrite sources mid-`cp -a`.
    let _lock = seeded_python_fixture_lock();
    let tmp = tempfile::TempDir::new().expect("seeded python tempdir");
    copy_repo_tree(&persistent_seeded_python_repo_unlocked(), tmp.path());
    tmp
}

pub struct LockedSeededPythonRepo {
    _tmp: tempfile::TempDir,
    path: PathBuf,
}

impl LockedSeededPythonRepo {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Isolated clone of the seeded python fixture.
pub fn locked_seeded_python_repo() -> LockedSeededPythonRepo {
    let tmp = fresh_seeded_python_repo();
    let path = tmp.path().to_path_buf();
    LockedSeededPythonRepo { _tmp: tmp, path }
}

pub fn parse_python_source(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, tmp.path()).expect("should parse temp source")
}

pub fn first_function_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "function_definition"
        {
            return node;
        }
    }

    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "decorated_definition"
        {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "function_definition" {
                    return child;
                }
            }
        }
    }

    panic!("function_definition");
}

pub fn first_function_or_async_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "function_definition" || n.kind() == "async_function_definition")
        .expect("function_definition")
}
