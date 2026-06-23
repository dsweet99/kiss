use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn kiss_output(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = kiss();
    cmd.current_dir(cwd);
    for arg in args {
        cmd.arg(*arg);
    }
    cmd.output().unwrap()
}

fn write_viz_module_fixture(dir: &std::path::Path) {
    fs::write(dir.join("a.py"), "import foo_bar\n").unwrap();
    fs::write(dir.join("foo_bar.py"), "import downstream_a\n").unwrap();
    fs::write(dir.join("foo-bar.py"), "import downstream_b\n").unwrap();
    fs::write(dir.join("downstream_a.py"), "X = 1\n").unwrap();
    fs::write(dir.join("downstream_b.py"), "Y = 2\n").unwrap();
}

fn run_viz(cwd: &std::path::Path, out: &std::path::Path) {
    kiss_output(
        cwd,
        &[
            "viz",
            "--defaults",
            "--zoom=1",
            out.to_str().unwrap(),
            ".",
        ],
    );
}

#[test]
fn regression_check_from_empty_cwd_uses_cli_path() {
    let root = TempDir::new().unwrap();
    let empty = root.path().join("empty_cwd");
    let repo = root.path().join("repo");
    fs::create_dir(&empty).unwrap();
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("a.py"), "def x(): pass\n").unwrap();

    let output = kiss()
        .current_dir(&empty)
        .args(["check", "../repo", "--defaults"])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("No source files found"),
        "expected check to analyze ../repo, got: {combined}"
    );
    assert!(
        combined.contains("a.py"),
        "expected repo file to be analyzed, got: {combined}"
    );
}

#[test]
fn regression_config_written_to_cli_path_not_cwd() {
    let root = TempDir::new().unwrap();
    let parent = root.path().join("parent");
    let child = parent.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::write(child.join("lib.py"), "def f(): pass\n").unwrap();

    let output = kiss()
        .current_dir(&parent)
        .args(["check", "child"])
        .output()
        .unwrap();
    let _ = output;
    assert!(
        child.join(".kissconfig").exists(),
        "config should be written under child/, not parent/"
    );
    assert!(
        !parent.join(".kissconfig").exists(),
        "config must not be written to cwd when checking child/"
    );
}

#[test]
fn regression_check_fails_on_python_syntax_error() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("bad.py"), "def foo( @@@\n").unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["check", "--defaults"])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "syntax error should fail check, got: {combined}"
    );
    assert!(
        combined.contains("syntax") || combined.contains("Syntax"),
        "expected syntax violation message, got: {combined}"
    );
}

#[test]
fn regression_shrink_check_accepts_single_path_argument() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("one.py"), "def f(): pass\n").unwrap();

    let start = kiss()
        .current_dir(tmp.path())
        .args(["shrink", "code_units=1", "--defaults"])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "shrink start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );

    let bad = kiss()
        .current_dir(tmp.path())
        .args(["shrink", "--defaults", "."])
        .output()
        .unwrap();
    assert!(
        !bad.status.success() || !String::from_utf8_lossy(&bad.stderr).contains("Invalid format"),
        "unexpected parse error: {}",
        String::from_utf8_lossy(&bad.stderr)
    );

    let check = kiss()
        .current_dir(tmp.path())
        .args(["shrink", "--defaults", "one.py"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        !stderr.contains("Invalid format"),
        "single path must not be parsed as METRIC=VALUE: {stderr}"
    );
}

#[test]
fn regression_check_lang_filter_exits_nonzero_when_no_matching_files() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "def x(): pass\n").unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["check", "--lang", "rust", "--defaults", "."])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "check --lang rust on Python-only tree should fail"
    );
}

#[test]
fn regression_fake_ignore_applies_to_stats() {
    let tmp = TempDir::new().unwrap();
    let fake = tmp.path().join("tests/fake_python");
    let real = tmp.path().join("real");
    fs::create_dir_all(&fake).unwrap();
    fs::create_dir(&real).unwrap();
    fs::write(fake.join("bad.py"), "def foo(a,b,c,d,e,f,g,h): pass\n").unwrap();
    fs::write(real.join("main.py"), "def ok(x): return x\n").unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["stats", "--all", "--defaults", "."])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("fake_python/bad.py"),
        "stats should ignore fake_ paths like check, got: {stdout}"
    );
    assert!(
        stdout.contains("real/main.py"),
        "stats should still analyze non-fake files, got: {stdout}"
    );
}

#[test]
fn regression_stats_deduplicates_duplicate_paths() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("small.py"), "def ok(x): return x\n").unwrap();
    fs::write(tmp.path().join("big.py"), "def f(a,b,c,d,e,f,g,h): pass\n").unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["stats", "--defaults", ".", "."])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Analyzed: 2 files"),
        "duplicate roots must not double-count files, got: {stdout}"
    );
}

#[test]
fn regression_stats_rejects_nonexistent_paths() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("good.py"), "def ok(x): return x\n").unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["stats", "--defaults", ".", "nonexistent"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "stats must reject nonexistent paths like check"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Path does not exist"),
        "expected path error message"
    );
}

#[test]
fn regression_defaults_does_not_write_kissconfig() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("big.py"), "def f(a,b,c,d,e,f,g,h): pass\n").unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["check", "--defaults", "."])
        .output()
        .unwrap();
    assert!(
        !tmp.path().join(".kissconfig").exists(),
        "--defaults must not create .kissconfig; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn regression_focus_coverage_gate_checks_universe_outside_focus() {
    let tmp = TempDir::new().unwrap();
    let good = tmp.path().join("good");
    let bad = tmp.path().join("bad");
    fs::create_dir_all(&good).unwrap();
    fs::create_dir_all(&bad).unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[gate]\ntest_coverage_threshold = 90\nduplication_enabled = false\norphan_module_enabled = false\n[python]\nfunctions_per_file = 100\nstatements_per_file = 1000\nlines_per_file = 1000\nimported_names_per_file = 100\n",
    )
    .unwrap();
    fs::write(good.join("lib.py"), "def covered(): return 1\n").unwrap();
    fs::write(bad.join("lib.py"), "def orphan(): pass\n").unwrap();
    fs::write(
        tmp.path().join("test_good.py"),
        "from good.lib import covered\n\ndef test_covered():\n    assert covered() == 1\n",
    )
    .unwrap();

    let output = kiss()
        .current_dir(tmp.path())
        .args(["check", "--lang", "python", ".", "good/"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !output.status.success(),
        "focus mode must still gate-check uncovered files outside focus, got: {stdout}"
    );
    assert!(
        stdout.contains("GATE_FAILED:test_coverage"),
        "expected coverage gate failure, got: {stdout}"
    );
}

#[test]
fn regression_check_expands_rust_includes_for_single_file() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let included = tmp.path().join("included");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&included).unwrap();
    fs::write(
        src.join("lib.rs"),
        "include!(\"../included/extra.inc\");\npub fn ok() -> i32 { 1 }\n",
    )
    .unwrap();
    fs::write(
        included.join("extra.inc"),
        "pub fn bad(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) -> i32 { a + b }\n",
    )
    .unwrap();

    let check = kiss()
        .current_dir(tmp.path())
        .args(["check", "--all", "--lang", "rust", "--defaults", "src/lib.rs"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        stdout.contains("Analyzed: 2 files"),
        "check must expand include! targets like stats, got: {stdout}"
    );
}

#[test]
fn regression_viz_mermaid_preserves_distinct_module_names() {
    let tmp = TempDir::new().unwrap();
    write_viz_module_fixture(tmp.path());

    let dot_path = tmp.path().join("graph.dot");
    let mmd_path = tmp.path().join("graph.mmd");
    run_viz(tmp.path(), &dot_path);
    run_viz(tmp.path(), &mmd_path);

    let dot = fs::read_to_string(&dot_path).unwrap();
    let mmd = fs::read_to_string(&mmd_path).unwrap();
    assert!(dot.contains("foo-bar"));
    assert!(dot.contains("foo_bar"));
    assert!(
        mmd.contains("foo_2d_bar["),
        "expected distinct mermaid id for foo-bar, got:\n{mmd}"
    );
    assert!(
        mmd.contains("foo_5f_bar["),
        "expected distinct mermaid id for foo_bar, got:\n{mmd}"
    );
}
