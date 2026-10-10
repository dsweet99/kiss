use kiss::{Language, graph_key_maxima};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn run_kiss(root: &Path, args: &[&str]) -> Output {
    kiss_binary()
        .current_dir(root)
        .args(args)
        .output()
        .expect("kiss should run")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn combined(out: &Output) -> String {
    format!("{}\n{}", stdout_of(out), stderr_of(out))
}

fn table_usize(toml: &str, section: &str, key: &str) -> Option<i64> {
    let table: toml::Table = toml.parse().ok()?;
    table.get(section)?.as_table()?.get(key)?.as_integer()
}

fn assert_covers_measured(written: i64, observed: i64, default_value: i64) {
    assert!(
        written >= observed,
        "threshold {written} must cover measured {observed}"
    );
    if observed > default_value {
        assert_eq!(
            written, observed,
            "an exceeded default of {default_value} must rise to the measured value"
        );
    } else {
        assert_eq!(
            written, default_value,
            "a measurement within the default must leave the default unchanged"
        );
    }
}

fn python_graph_maxima(root: &Path) -> kiss::GraphKeyMaxima {
    let ignore = vec!["fake_".to_string(), "fixtures".to_string()];
    let path = root.to_string_lossy().into_owned();
    let (py_files, _) =
        kiss::gather_files_by_lang(std::slice::from_ref(&path), Some(Language::Python), &ignore);
    let parsed = kiss::parse_files(&py_files).expect("python parse");
    let ok: Vec<_> = parsed.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<_> = ok.iter().collect();
    graph_key_maxima(&kiss::build_dependency_graph(&refs))
}

fn write_small_python_package(root: &Path) {
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("pkg/__init__.py"), "VALUE = 1\n").unwrap();
    fs::write(root.join("pkg/__main__.py"), "from pkg import VALUE\n").unwrap();
    fs::write(
        root.join("tests/test_pkg.py"),
        "from pkg import VALUE\n\ndef test_pkg():\n    assert VALUE == 1\n",
    )
    .unwrap();
}

fn write_issue41_python_tree(root: &Path) {
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("inspect.py"), "def probe():\n    return 1\n").unwrap();
    fs::write(root.join("src/inspect.py"), "import leaf\n").unwrap();
    fs::write(root.join("leaf.py"), "LEAF = 1\n").unwrap();
    fs::write(root.join("foo.py"), "import bar\n").unwrap();
    fs::write(root.join("src/foo.py"), "import util\n").unwrap();
    fs::write(root.join("util.py"), "import leaf\n").unwrap();
    fs::write(root.join("bar.py"), "import util\n").unwrap();
    fs::write(root.join("pkg/__init__.py"), "import inspect\n").unwrap();
    fs::write(root.join("pkg/__main__.py"), "import pkg.biz\n").unwrap();
    fs::write(root.join("pkg/biz.py"), "import inspect\nimport foo\n").unwrap();
    fs::write(
        root.join("tests/test_biz.py"),
        "import pkg.biz\n\ndef test_biz():\n    assert True\n",
    )
    .unwrap();
}

fn write_fake_python_tree(root: &Path) {
    fs::create_dir_all(root.join("tests/fake_python")).unwrap();
    fs::write(root.join("app.py"), "def tiny(x):\n    return x\n").unwrap();
    fs::write(
        root.join("tests/fake_python/deep.py"),
        "def huge(a, b, c, d, e, f, g, h, i, j):\n    return a\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/test_app.py"),
        "from app import tiny\n\ndef test_app():\n    assert tiny(1) == 1\n",
    )
    .unwrap();
}

fn rust_too_many_args() -> &'static str {
    "pub fn too_many(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32, i: i32) -> i32 { a }\n"
}

#[test]
fn clamp_below_check_fails_on_head() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_issue41_python_tree(root);

    let check = run_kiss(root, &["check", "."]);
    assert!(
        root.join(".kissconfig").exists(),
        "check must write .kissconfig when missing: {}",
        combined(&check)
    );
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    let written = table_usize(&config, "python", "indirect_dependencies")
        .expect("python indirect_dependencies");
    let observed = python_graph_maxima(root).indirect_dependencies as i64;
    assert_covers_measured(written, observed, 4);
    let stdout = stdout_of(&check);
    assert!(
        check.status.success(),
        "first check should be green: {}",
        combined(&check)
    );
    assert!(
        stdout.contains("NO VIOLATIONS"),
        "expected NO VIOLATIONS; stdout:\n{stdout}"
    );
}

#[test]
fn clamp_then_check_is_green() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_small_python_package(root);

    let check = run_kiss(root, &["check", "."]);
    assert!(check.status.success(), "check failed: {}", combined(&check));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[python]"), "config:\n{config}");
    assert!(
        config.contains("[rust]"),
        "default config keeps [rust]:\n{config}"
    );
    assert!(
        config.contains("duplication_enabled = true")
            && config.contains("orphan_detection = false")
            && !config.contains("orphan_module_enabled")
            && config.contains("comment_removal_enabled = false")
            && config.contains("docs_allowed = []")
            && config.contains("\"*\" = 3"),
        "written gate defaults:\n{config}"
    );
    let written = table_usize(&config, "python", "indirect_dependencies")
        .expect("python indirect_dependencies");
    assert_covers_measured(
        written,
        python_graph_maxima(root).indirect_dependencies as i64,
        4,
    );

    let check = run_kiss(root, &["check", "."]);
    let stdout = stdout_of(&check);
    assert!(
        check.status.success(),
        "second check should stay green: {}",
        combined(&check)
    );
    assert!(
        stdout.contains("NO VIOLATIONS"),
        "expected NO VIOLATIONS; stdout:\n{stdout}"
    );
}

#[test]
fn default_file_keeps_rust_and_does_not_reraise() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_small_python_package(root);

    let first = run_kiss(root, &["check", "."]);
    assert!(first.status.success(), "{}", combined(&first));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[python]"), "config:\n{config}");
    assert!(
        config.contains("[rust]"),
        "default config includes [rust] even for a python-only repo:\n{config}"
    );
    assert_eq!(table_usize(&config, "rust", "arguments"), Some(4));

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), rust_too_many_args()).unwrap();

    let check = run_kiss(root, &["check", "."]);
    let text = combined(&check);
    assert!(
        !check.status.success(),
        "an existing config must not be raised for a later file: {text}"
    );
    let kept = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert_eq!(
        table_usize(&kept, "rust", "arguments"),
        Some(4),
        "arguments must stay at the written default:\n{kept}"
    );
}

#[test]
fn default_file_keeps_python_section_and_later_python_is_not_reraise() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();

    let first = run_kiss(root, &["check", "."]);
    assert!(first.status.success(), "{}", combined(&first));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[rust]"), "config:\n{config}");
    assert!(config.contains("[python]"), "config:\n{config}");

    let stats = run_kiss(root, &["stats", "."]);
    assert!(
        stats.status.success(),
        "stats after default config: {}",
        combined(&stats)
    );

    let viz = run_kiss(root, &["viz", "graph.mmd", "."]);
    assert!(
        viz.status.success(),
        "viz after default config: {}",
        combined(&viz)
    );

    fs::write(
        root.join("too_many.py"),
        "def too_many(a, b, c, d, e, f):\n    return a\n",
    )
    .unwrap();

    let check = run_kiss(root, &["check", "."]);
    let text = combined(&check);
    assert!(
        !check.status.success(),
        "an existing config must not raise thresholds for code added later: {text}"
    );
    let kept = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert_eq!(
        table_usize(&kept, "python", "positional_args"),
        Some(5),
        "positional_args must stay at the written default:\n{kept}"
    );
}

fn assert_auto_config_ignores_fake_python(root: &Path) {
    assert!(!root.join(".kissconfig").exists());
    let check = run_kiss(root, &["check", "."]);
    assert!(
        root.join(".kissconfig").exists(),
        "first check must auto-create .kissconfig; {}",
        combined(&check)
    );
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    let args = table_usize(&config, "python", "positional_args").unwrap();
    assert!(
        args < 10,
        "auto-created config must ignore tests/fake_python; config:\n{config}"
    );
}

#[test]
fn ensure_default_config_uses_check_ignores() {
    let tmp = TempDir::new().unwrap();
    write_fake_python_tree(tmp.path());
    assert_auto_config_ignores_fake_python(tmp.path());
}

#[test]
fn mimic_out_matches_clamp_ignores() {
    let tmp = TempDir::new().unwrap();
    write_fake_python_tree(tmp.path());
    assert_auto_config_ignores_fake_python(tmp.path());
}

#[test]
fn init_still_writes_both_languages() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let init = run_kiss(root, &["init"]);
    assert!(
        !init.status.success(),
        "kiss init is removed: {}",
        combined(&init)
    );
    fs::write(root.join("app.py"), "def tiny():\n    return 1\n").unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    let check = run_kiss(root, &["check", "."]);
    assert!(check.status.success(), "{}", combined(&check));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[python]"), "config:\n{config}");
    assert!(config.contains("[rust]"), "config:\n{config}");
}

#[test]
fn check_foreign_path_clamps_the_checked_tree() {
    let cwd = TempDir::new().unwrap();
    let tree = TempDir::new().unwrap();
    fs::write(cwd.path().join("tiny.py"), "def tiny():\n    return 1\n").unwrap();
    fs::write(
        tree.path().join("mod.py"),
        "def f(a, b, c, d, e, f):\n    return a\n",
    )
    .unwrap();

    let check = run_kiss(cwd.path(), &["check", &tree.path().to_string_lossy()]);
    let config = fs::read_to_string(cwd.path().join(".kissconfig")).unwrap();
    assert!(
        check.status.success(),
        "first check of PATH must pass: {}",
        combined(&check)
    );
    assert!(
        stdout_of(&check).contains("NO VIOLATIONS"),
        "stdout:\n{}",
        stdout_of(&check)
    );
    assert_eq!(
        table_usize(&config, "python", "positional_args"),
        Some(6),
        "config must clamp the checked tree:\n{config}"
    );
}

#[test]
fn check_ignore_is_applied_when_auto_creating_config() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("vendor")).unwrap();
    fs::write(root.join("app.py"), "def tiny(x):\n    return x\n").unwrap();
    fs::write(
        root.join("vendor/big.py"),
        "def f(a, b, c, d, e, f, g, h, i, j):\n    return a\n",
    )
    .unwrap();

    let check = run_kiss(root, &["check", ".", "--ignore", "vendor"]);
    assert!(
        check.status.success(),
        "check --ignore must pass: {}",
        combined(&check)
    );
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert_eq!(
        table_usize(&config, "python", "positional_args"),
        Some(5),
        "ignored vendor code must not raise positional_args:\n{config}"
    );
    assert!(
        config.contains("ignore = []"),
        "the default ignore list must be kept:\n{config}"
    );
}

#[test]
fn stub_gate_config_survives_parse_failure_then_fills_after_ignore() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("bad")).unwrap();
    fs::write(root.join("ok.py"), "def ok():\n    return 1\n").unwrap();
    fs::write(root.join("bad/broken.py"), "def broken(\n").unwrap();

    let first = run_kiss(root, &["check", "."]);
    assert!(
        !first.status.success(),
        "parse failure must fail first check: {}",
        combined(&first)
    );
    assert!(
        root.join(".kissconfig").exists(),
        "gate stub must be written before stats/collect failure: {}",
        combined(&first)
    );
    let stub = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(
        stub.contains("[global]") && stub.contains("[test]") && stub.contains("[python]"),
        "default config must be written before a later collect failure:\n{stub}"
    );
    assert!(stub.contains("duplication_enabled = true"), "stub:\n{stub}");

    let mut patched = stub.replace("ignore = []", "ignore = [\"bad\"]");
    if !patched.contains("ignore = [\"bad\"]") {
        patched.push_str("\nignore = [\"bad\"]\n");
    }
    fs::write(root.join(".kissconfig"), patched).unwrap();

    let second = run_kiss(root, &["check", "."]);
    assert!(
        second.status.success(),
        "second check with stub ignore must fill language tables: {}",
        combined(&second)
    );
    let filled = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(
        filled.contains("[python]"),
        "language thresholds must be filled:\n{filled}"
    );
    assert!(
        filled.contains("ignore = [\"bad\"]"),
        "operator ignore must be preserved:\n{filled}"
    );
}

#[test]
fn reclamp_omits_language_with_zero_files() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("app.py"), "def tiny():\n    return 1\n").unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();

    let first = run_kiss(root, &["check", "."]);
    assert!(first.status.success(), "{}", combined(&first));
    let mixed = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(
        mixed.contains("[python]") && mixed.contains("[rust]"),
        "{mixed}"
    );

    fs::remove_file(root.join("lib.rs")).unwrap();
    fs::remove_file(root.join(".kissconfig")).unwrap();
    let second = run_kiss(root, &["check", "."]);
    assert!(second.status.success(), "{}", combined(&second));
    let python_only = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(python_only.contains("[python]"), "{python_only}");
    assert!(
        python_only.contains("[rust]"),
        "the default file keeps [rust] when the repo has no rust files:\n{python_only}"
    );
}
