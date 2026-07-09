use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use kiss::Language;

use crate::test_git::{TestChangeMode, git_command};

use super::ValidateSelectionCmdArgs;
use crate::test_runner::{plan_selectors, runners};

pub(crate) struct TinyRecallReport {
    selected_count: usize,
    full_count: usize,
    selected_failing: BTreeSet<String>,
    full_failing: BTreeSet<String>,
}

impl TinyRecallReport {
    fn new(
        selected_count: usize,
        full_count: usize,
        selected_failing: BTreeSet<String>,
        full_failing: BTreeSet<String>,
    ) -> Self {
        Self {
            selected_count,
            full_count,
            selected_failing,
            full_failing,
        }
    }

    fn missing_failing_selectors(&self) -> BTreeSet<String> {
        self.full_failing
            .difference(&self.selected_failing)
            .cloned()
            .collect()
    }

    pub(crate) fn has_full_recall(&self) -> bool {
        self.missing_failing_selectors().is_empty()
    }

    pub(crate) fn print(&self) {
        let missing = self.missing_failing_selectors();
        println!("KISS TEST VALIDATION FIXTURE");
        println!("fixture=tiny-recall");
        println!("selected_total={}", self.selected_count);
        println!("full_total={}", self.full_count);
        println!("selected_failing_total={}", self.selected_failing.len());
        println!("full_failing_total={}", self.full_failing.len());
        println!("missing_failing_total={}", missing.len());
        for selector in missing {
            println!("missing_failing_selector={selector}");
        }
    }
}

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new() -> Result<Self, String> {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("error: tiny-recall clock failure: {e}"))?
            .as_nanos();
        root.push(format!("kiss-tiny-recall-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root)
            .map_err(|e| format!("error: tiny-recall create temp repo: {e}"))?;
        Ok(Self { root })
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    fn change_to(path: &Path) -> Result<Self, String> {
        let original =
            std::env::current_dir().map_err(|e| format!("error: tiny-recall cwd: {e}"))?;
        std::env::set_current_dir(path)
            .map_err(|e| format!("error: tiny-recall enter fixture: {e}"))?;
        Ok(Self { original })
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

struct SelectorSets {
    python: BTreeSet<String>,
    rust: BTreeSet<String>,
}

impl SelectorSets {
    fn total(&self) -> usize {
        self.python.len() + self.rust.len()
    }

    fn failing_selectors(
        &self,
        repo_root: &Path,
        extra: &[String],
    ) -> Result<BTreeSet<String>, String> {
        let mut out = BTreeSet::new();
        for selector in &self.python {
            if selector_fails(python_command(repo_root, selector, extra))? {
                out.insert(format!("python:{selector}"));
            }
        }
        for selector in &self.rust {
            if selector_fails(rust_command(repo_root, selector, extra))? {
                out.insert(format!("rust:{selector}"));
            }
        }
        Ok(out)
    }
}

pub(crate) fn run_tiny_recall_fixture(
    args: &ValidateSelectionCmdArgs<'_>,
) -> Result<TinyRecallReport, String> {
    assert_eq!(args.fixture_name(), Some("tiny-recall"));
    let fixture = TempFixture::new()?;
    write_fixture_baseline(&fixture.root)?;
    init_fixture_git(&fixture.root)?;
    write_fixture_regression(&fixture.root)?;
    let full = full_selector_sets(&fixture.root, args)?;
    let selected = selected_selector_sets(&fixture.root, &full, args)?;
    let full_failing = full.failing_selectors(&fixture.root, args.planning_extra_args())?;
    let selected_failing = selected.failing_selectors(&fixture.root, args.planning_extra_args())?;
    Ok(TinyRecallReport::new(
        selected.total(),
        full.total(),
        selected_failing,
        full_failing,
    ))
}

fn write_fixture_baseline(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root.join("src")).map_err(|e| e.to_string())?;
    fs::create_dir_all(root.join("tests")).map_err(|e| e.to_string())?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"tiny_recall\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .map_err(|e| e.to_string())?;
    fs::write(root.join("calc.py"), "def value():\n    return 2\n").map_err(|e| e.to_string())?;
    fs::write(
        root.join("tests").join("test_calc.py"),
        "from calc import value\n\n\
def test_value_tracks_source():\n    assert value() == 2\n\n\
def test_python_unrelated_passes():\n    assert True\n",
    )
    .map_err(|e| e.to_string())?;
    write_rust_lib(root, 2)
}

fn write_fixture_regression(root: &Path) -> Result<(), String> {
    fs::write(root.join("calc.py"), "def value():\n    return 3\n").map_err(|e| e.to_string())?;
    write_rust_lib(root, 3)
}

fn write_rust_lib(root: &Path, value: u32) -> Result<(), String> {
    fs::write(
        root.join("src").join("lib.rs"),
        format!(
            "pub fn rust_value() -> u32 {{ {value} }}\n\n\
#[cfg(test)]\n\
mod tests {{\n\
    use super::*;\n\n\
    #[test]\n\
    fn rust_value_tracks_source() {{\n\
        assert_eq!(rust_value(), 2);\n\
    }}\n\n\
    #[test]\n\
    fn rust_unrelated_passes() {{\n\
        assert_eq!(1, 1);\n\
    }}\n\
}}\n"
        ),
    )
    .map_err(|e| e.to_string())
}

fn init_fixture_git(root: &Path) -> Result<(), String> {
    git_ok(root, &["init"])?;
    git_ok(
        root,
        &["config", "user.email", "tiny-recall@example.invalid"],
    )?;
    git_ok(root, &["config", "user.name", "tiny recall"])?;
    git_ok(root, &["add", "."])?;
    git_ok(root, &["commit", "-m", "baseline"])?;
    Ok(())
}

fn git_ok(root: &Path, args: &[&str]) -> Result<(), String> {
    let output = git_command(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("error: tiny-recall git failed to start: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "error: tiny-recall git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn full_selector_sets(
    repo_root: &Path,
    args: &ValidateSelectionCmdArgs<'_>,
) -> Result<SelectorSets, String> {
    let include_python = args.normalized_lang_filter() != Some(Language::Rust);
    let include_rust = args.normalized_lang_filter() != Some(Language::Python);
    Ok(SelectorSets {
        python: selectors_if(include_python, || {
            runners::enumerate_workspace_python_selectors(repo_root, args.planning_ignore_args())
        })?,
        rust: selectors_if(include_rust, || {
            runners::enumerate_workspace_rust_selectors(repo_root, args.planning_ignore_args())
        })?,
    })
}

fn selected_selector_sets(
    repo_root: &Path,
    full: &SelectorSets,
    args: &ValidateSelectionCmdArgs<'_>,
) -> Result<SelectorSets, String> {
    let _cwd = CwdGuard::change_to(repo_root)?;
    let planned = plan_selectors(
        TestChangeMode::Commit,
        None,
        None,
        args.planning_ignore_args(),
        args.planning_extra_args(),
        args.normalized_lang_filter(),
        None,
    )?;
    let mut python = planned.py_sel.into_iter().collect::<BTreeSet<_>>();
    if planned.python_population_required {
        python.extend(full.python.iter().cloned());
    }
    let mut rust = planned.rs_sel.into_iter().collect::<BTreeSet<_>>();
    if !planned.rust_source_population_paths.is_empty() {
        rust.extend(planned.rust_population_selectors);
    }
    Ok(SelectorSets { python, rust })
}

fn selectors_if<F>(enabled: bool, f: F) -> Result<BTreeSet<String>, String>
where
    F: FnOnce() -> Result<Vec<String>, String>,
{
    if enabled {
        Ok(f()?.into_iter().collect())
    } else {
        Ok(BTreeSet::new())
    }
}

fn python_command(repo_root: &Path, selector: &str, extra: &[String]) -> Command {
    let mut command = Command::new("python");
    command
        .current_dir(repo_root)
        .env("PYTHONPATH", repo_root)
        .args(["-m", "pytest", "-q", selector])
        .args(extra)
        .stdin(Stdio::null());
    command
}

fn rust_command(repo_root: &Path, selector: &str, extra: &[String]) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(repo_root)
        .args(["test", "--quiet", selector, "--"])
        .args(extra)
        .stdin(Stdio::null());
    command
}

fn selector_fails(mut command: Command) -> Result<bool, String> {
    let output = command
        .output()
        .map_err(|e| format!("error: tiny-recall selector failed to start: {e}"))?;
    Ok(!output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_detects_missing_failing_selector() {
        let report = TinyRecallReport::new(
            1,
            2,
            BTreeSet::from(["python:test_a.py::test_a".to_string()]),
            BTreeSet::from([
                "python:test_a.py::test_a".to_string(),
                "rust:tests::test_b".to_string(),
            ]),
        );

        assert!(!report.has_full_recall());
        assert_eq!(
            report.missing_failing_selectors(),
            BTreeSet::from(["rust:tests::test_b".to_string()])
        );
    }
}
