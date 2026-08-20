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

fn disable_coverage_gate(root: &Path) {
    let path = root.join(".kissconfig");
    let config = fs::read_to_string(&path).unwrap();
    let config = config.replace(
        "test_coverage_threshold = 90",
        "test_coverage_threshold = 0",
    );
    fs::write(path, config).unwrap();
}

fn reclamp_message(language: &str) -> String {
    format!(
        "Error: found {language} files but .kissconfig has no [{language}] table. Run `kiss clamp` to generate language thresholds."
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

    let clamp = run_kiss(root, &["clamp"]);
    assert!(clamp.status.success(), "clamp failed: {}", combined(&clamp));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    let written = table_usize(&config, "python", "indirect_dependencies")
        .expect("python indirect_dependencies");
    let observed = python_graph_maxima(root).indirect_dependencies as i64;
    assert_eq!(
        written, observed,
        "clamp must write check's graph max; config:\n{config}"
    );

    disable_coverage_gate(root);
    let check = run_kiss(root, &["check", "."]);
    let stdout = stdout_of(&check);
    assert!(
        check.status.success(),
        "check after clamp should be green: {}",
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

    let clamp = run_kiss(root, &["clamp"]);
    assert!(clamp.status.success(), "clamp failed: {}", combined(&clamp));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[python]"), "config:\n{config}");
    assert!(
        !config.contains("[rust]"),
        "python-only clamp must omit rust:\n{config}"
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
        "check after clamp should be green: {}",
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

    let clamp = run_kiss(root, &["clamp"]);
    assert!(clamp.status.success(), "{}", combined(&clamp));
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

    let clamp = run_kiss(root, &["clamp"]);
    assert!(clamp.status.success(), "{}", combined(&clamp));
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

#[test]
fn mimic_out_matches_clamp_ignores() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_fake_python_tree(root);

    let mimic = run_kiss(root, &["mimic", ".", "--out", ".kissconfig"]);
    assert!(mimic.status.success(), "{}", combined(&mimic));
    let mimic_cfg = fs::read_to_string(root.join(".kissconfig")).unwrap();
    let mimic_args = table_usize(&mimic_cfg, "python", "positional_args").unwrap();
    assert!(
        mimic_args < 10,
        "mimic must ignore tests/fake_python; config:\n{mimic_cfg}"
    );

    fs::remove_file(root.join(".kissconfig")).unwrap();
    let clamp = run_kiss(root, &["clamp"]);
    assert!(clamp.status.success(), "{}", combined(&clamp));
    let clamp_cfg = fs::read_to_string(root.join(".kissconfig")).unwrap();
    let clamp_args = table_usize(&clamp_cfg, "python", "positional_args").unwrap();
    assert_eq!(mimic_args, clamp_args);
    assert!(
        clamp_args < 10,
        "clamp must ignore tests/fake_python; config:\n{clamp_cfg}"
    );
}

#[test]
fn ensure_default_config_uses_check_ignores() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_fake_python_tree(root);
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
fn init_still_writes_both_languages() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let init = run_kiss(root, &["init"]);
    assert!(init.status.success(), "{}", combined(&init));
    let config = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(config.contains("[python]"), "config:\n{config}");
    assert!(config.contains("[rust]"), "config:\n{config}");
    assert_eq!(config, kiss::default_config_toml());
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

    let first = run_kiss(root, &["clamp"]);
    assert!(first.status.success(), "{}", combined(&first));
    let mixed = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(
        mixed.contains("[python]") && mixed.contains("[rust]"),
        "{mixed}"
    );

    fs::remove_file(root.join("lib.rs")).unwrap();
    let second = run_kiss(root, &["clamp"]);
    assert!(second.status.success(), "{}", combined(&second));
    let python_only = fs::read_to_string(root.join(".kissconfig")).unwrap();
    assert!(python_only.contains("[python]"), "{python_only}");
    assert!(
        !python_only.contains("[rust]"),
        "re-clamp must omit rust when no rust files remain:\n{python_only}"
    );
}
