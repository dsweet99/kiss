use crate::common::seed_python_runtime_coverage;
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

fn seed_all_python(root: &Path, selector: &str) {
    let mut owned: Vec<(String, Vec<u32>)> = Vec::new();
    collect_py_coverage_files(root, root, &mut owned);
    let files: Vec<(&str, Vec<u32>)> = owned
        .iter()
        .map(|(path, lines)| (path.as_str(), lines.clone()))
        .collect();
    seed_python_runtime_coverage(root, &[(selector, files)]);
}

fn collect_py_coverage_files(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u32>)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_py_coverage_files(root, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("py") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let n = fs::read_to_string(&path).unwrap().lines().count() as u32;
        out.push((rel, (1..=n.max(1)).collect()));
    }
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
    seed_all_python(root, "tests/test_pkg.py::test_pkg");
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
    seed_all_python(root, "tests/test_app.py::test_app");
}

fn reclamp_message(language: &str) -> String {
    format!(
        "Error: found {language} files but .kissconfig has no [{language}] table. Delete .kissconfig and run `kiss check` to generate language thresholds."
    )
}

fn rust_too_many_args() -> &'static str {
    "pub fn too_many(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32, i: i32) -> i32 { a }\n"
}

fn python_too_many_args() -> &'static str {
    "def too_many(a, b, c, d):\n    return a\n"
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
    assert_eq!(
        written, observed,
        "check must write its own graph max; config:\n{config}"
    );
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
        !config.contains("[rust]"),
        "python-only check must omit rust:\n{config}"
    );
    assert!(
        config.contains("duplication_enabled = false")
            && config.contains("orphan_module_enabled = false")
            && config.contains("comment_removal_enabled = false")
            && config.contains("docs_allowed = []")
            && config.contains("test_coverage_threshold = 0")
            && config.contains("\"*\" = 99999"),
        "auto-created gate defaults:\n{config}"
    );
    let written = table_usize(&config, "python", "indirect_dependencies")
        .expect("python indirect_dependencies");
    assert_eq!(
        written,
        python_graph_maxima(root).indirect_dependencies as i64
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
fn python_only_clamp_omits_rust() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_small_python_package(root);

    let first = run_kiss(root, &["check", "."]);
    assert!(first.status.success(), "{}", combined(&first));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[python]"));
    assert!(!config.contains("[rust]"), "config:\n{config}");

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), rust_too_many_args()).unwrap();

    let check = run_kiss(root, &["check", "."]);
    let text = combined(&check);
    assert!(!check.status.success(), "check must fail: {text}");
    assert!(
        text.contains(&reclamp_message("rust")),
        "expected re-clamp message; output:\n{text}"
    );
    assert!(
        !text.contains("VIOLATION:positional_args"),
        "must not score rust with stock arguments=8; output:\n{text}"
    );

    let stats = run_kiss(root, &["stats", "."]);
    let stats_text = combined(&stats);
    assert!(!stats.status.success(), "stats must fail: {stats_text}");
    assert!(
        stats_text.contains(&reclamp_message("rust")),
        "stats output:\n{stats_text}"
    );
    assert!(!stats_text.contains("VIOLATION:positional_args"));

    let viz = run_kiss(root, &["viz", "graph.mmd", "."]);
    let viz_text = combined(&viz);
    assert!(!viz.status.success(), "viz must fail: {viz_text}");
    assert!(
        viz_text.contains(&reclamp_message("rust")),
        "viz output:\n{viz_text}"
    );
    assert!(
        !viz_text.contains("Error: Error:"),
        "viz must not double Error prefix:\n{viz_text}"
    );
}

#[test]
fn rust_only_clamp_omits_python() {
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
    assert!(!config.contains("[python]"), "config:\n{config}");

    fs::write(root.join("too_many.py"), python_too_many_args()).unwrap();

    let check = run_kiss(root, &["check", "."]);
    let text = combined(&check);
    assert!(!check.status.success(), "check must fail: {text}");
    assert!(
        text.contains(&reclamp_message("python")),
        "expected re-clamp message; output:\n{text}"
    );
    assert!(
        !text.contains("VIOLATION:positional_args"),
        "must not score python with stock positional_args=3; output:\n{text}"
    );

    let stats = run_kiss(root, &["stats", "."]);
    let stats_text = combined(&stats);
    assert!(!stats.status.success());
    assert!(stats_text.contains(&reclamp_message("python")));
    assert!(!stats_text.contains("VIOLATION:positional_args"));

    let viz = run_kiss(root, &["viz", "graph.mmd", "."]);
    let viz_text = combined(&viz);
    assert!(!viz.status.success(), "viz must fail: {viz_text}");
    assert!(
        viz_text.contains(&reclamp_message("python")),
        "viz output:\n{viz_text}"
    );
    assert!(
        !viz_text.contains("Error: Error:"),
        "viz must not double Error prefix:\n{viz_text}"
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
        Some(1),
        "auto-create must honor --ignore:\n{config}"
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
        !python_only.contains("[rust]"),
        "regenerated config must omit rust when no rust files remain:\n{python_only}"
    );
}
