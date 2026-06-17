use crate::rust_parsing::ParsedRustFile;
use crate::rust_test_refs::{RustCodeDefinition, RustTestRefAnalysis};
use crate::units::CodeUnitKind;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustLineCoverage {
    pub file: PathBuf,
    pub executable_lines: Vec<usize>,
    pub missing_lines: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoLlvmCovCommand {
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn normalize_against(repo_root: &Path, filename: &str) -> PathBuf {
    let path = PathBuf::from(filename);
    path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf()
}

fn path_has_component(path: &Path, name: &str) -> bool {
    path.components()
        .any(|c| matches!(c, Component::Normal(part) if part == name))
}

fn is_rust_test_module_path(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.starts_with("test_")
        || stem.starts_with("tests_")
}

fn is_rust_product_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "rs")
        && !path_has_component(path, "tests")
        && !path_has_component(path, "target")
        && !is_rust_test_module_path(path)
}

fn lines_from_segments(segments: &[Value]) -> (Vec<usize>, Vec<usize>) {
    let mut line_covered = BTreeMap::<usize, bool>::new();
    for segment in segments {
        let Some(items) = segment.as_array() else {
            continue;
        };
        let Some(line) = items
            .first()
            .and_then(Value::as_u64)
            .and_then(|line| usize::try_from(line).ok())
        else {
            continue;
        };
        let Some(count) = items.get(2).and_then(Value::as_u64) else {
            continue;
        };
        if !items.get(3).and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        line_covered
            .entry(line)
            .and_modify(|covered| *covered |= count > 0)
            .or_insert(count > 0);
    }
    let executable_lines = line_covered.keys().copied().collect::<Vec<_>>();
    let missing_lines = line_covered
        .into_iter()
        .filter_map(|(line, covered)| (!covered).then_some(line))
        .collect::<Vec<_>>();
    (executable_lines, missing_lines)
}

pub fn parse_llvm_cov_json(
    repo_root: &Path,
    payload: &str,
) -> Result<Vec<RustLineCoverage>, String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let value: Value = serde_json::from_str(payload).map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for item in value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for file_info in item
            .get("files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(filename) = file_info.get("filename").and_then(Value::as_str) else {
                continue;
            };
            let rel = normalize_against(&root, filename);
            if !is_rust_product_path(&rel) {
                continue;
            }
            let Some(segments) = file_info.get("segments").and_then(Value::as_array) else {
                continue;
            };
            let (executable_lines, missing_lines) = lines_from_segments(segments);
            if executable_lines.is_empty() {
                continue;
            }
            out.push(RustLineCoverage {
                file: rel,
                executable_lines,
                missing_lines,
            });
        }
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

fn repo_requires_nextest(repo_root: &Path) -> bool {
    repo_root.join("nextest.toml").is_file()
        || repo_root.join(".config/nextest.toml").is_file()
        || std::fs::read_to_string(repo_root.join(".malvin/checks"))
            .is_ok_and(|checks| checks.lines().any(|line| line.contains("cargo nextest")))
}

pub fn cargo_llvm_cov_command(repo_root: &Path, output_path: &Path) -> CargoLlvmCovCommand {
    let mut args = vec!["llvm-cov".to_string()];
    if repo_requires_nextest(repo_root) {
        args.push("nextest".to_string());
    }
    args.push("--workspace".to_string());
    args.extend([
        "--json".to_string(),
        "--output-path".to_string(),
        output_path.to_string_lossy().to_string(),
    ]);
    CargoLlvmCovCommand {
        cwd: repo_root.to_path_buf(),
        args,
        env: vec![("RUST_TEST_THREADS".to_string(), "1".to_string())],
    }
}

fn hash_bytes(hasher: &mut std::collections::hash_map::DefaultHasher, bytes: &[u8]) {
    hasher.write(bytes);
}

fn command_version(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) => {
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            if text.trim().is_empty() {
                format!("status:{}", output.status)
            } else {
                text.trim().to_string()
            }
        }
        Err(err) => format!("ERROR:{err}"),
    }
}

pub fn backend_fingerprint(repo_root: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let placeholder = Path::new("$KISS_LLVM_COV_JSON");
    let command = cargo_llvm_cov_command(repo_root, placeholder);
    hash_bytes(&mut hasher, b"rust-llvm-cov-v1");
    for arg in &command.args {
        hash_bytes(&mut hasher, arg.as_bytes());
        hash_bytes(&mut hasher, b"\0");
    }
    for rel in ["Cargo.toml", "Cargo.lock"] {
        hash_bytes(&mut hasher, rel.as_bytes());
        hash_bytes(&mut hasher, b"=");
        if let Ok(bytes) = std::fs::read(repo_root.join(rel)) {
            hash_bytes(&mut hasher, &bytes);
        }
        hash_bytes(&mut hasher, b"\0");
    }
    for (key, value) in std::env::vars().filter(|(key, _)| {
        matches!(
            key.as_str(),
            "RUSTDOCFLAGS" | "CARGO_TARGET_DIR" | "CARGO_INCREMENTAL"
        )
    }) {
        hash_bytes(&mut hasher, key.as_bytes());
        hash_bytes(&mut hasher, b"=");
        hash_bytes(&mut hasher, value.as_bytes());
        hash_bytes(&mut hasher, b"\0");
    }
    for version in [
        command_version("cargo", &["--version"]),
        command_version("cargo", &["llvm-cov", "--version"]),
        command_version("rustc", &["--version"]),
    ] {
        hash_bytes(&mut hasher, version.as_bytes());
        hash_bytes(&mut hasher, b"\0");
    }
    if repo_requires_nextest(repo_root) {
        hash_bytes(
            &mut hasher,
            command_version("cargo", &["nextest", "--version"]).as_bytes(),
        );
    }
    format!("{:016x}", hasher.finish())
}

fn temp_output_path() -> PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("kiss-llvm-cov-{}-{now}.json", std::process::id()))
}

pub fn collect_cargo_llvm_cov(repo_root: &Path) -> Result<Vec<RustLineCoverage>, String> {
    let output_path = temp_output_path();
    let cmd = cargo_llvm_cov_command(repo_root, &output_path);
    let mut command = Command::new("cargo");
    command.args(&cmd.args).current_dir(&cmd.cwd);
    for (key, value) in &cmd.env {
        command.env(key, value);
    }
    for key in rust_coverage_env_keys_to_remove() {
        command.env_remove(key);
    }
    let result = command
        .output()
        .map_err(|err| format!("failed to run cargo llvm-cov: {err}"))?;
    if !result.status.success() {
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        let _ = std::fs::remove_file(&output_path);
        return Err(format!(
            "cargo llvm-cov failed\ncommand: cargo {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            cmd.args.join(" ")
        ));
    }
    let payload = std::fs::read_to_string(&output_path)
        .map_err(|err| format!("cargo llvm-cov did not write coverage JSON: {err}"))?;
    let _ = std::fs::remove_file(&output_path);
    parse_llvm_cov_json(repo_root, &payload)
}

fn rust_coverage_env_keys_to_remove() -> &'static [&'static str] {
    &[
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "LLVM_PROFILE_FILE",
        "CARGO_LLVM_COV",
        "CARGO_LLVM_COV_TARGET_DIR",
    ]
}

fn empty_analysis() -> RustTestRefAnalysis {
    RustTestRefAnalysis {
        definitions: Vec::new(),
        test_references: HashSet::new(),
        call_references: HashSet::new(),
        propagated_references: HashSet::new(),
        unreferenced: Vec::new(),
        coverage_map: HashMap::new(),
    }
}

fn is_nested_cargo_llvm_cov_run() -> bool {
    std::env::var_os("CARGO_LLVM_COV").is_some()
        || std::env::var_os("CARGO_LLVM_COV_TARGET_DIR").is_some()
}

fn line_definition(file: PathBuf, line: usize) -> RustCodeDefinition {
    RustCodeDefinition {
        name: format!("line_{line}"),
        kind: CodeUnitKind::Module,
        file,
        line,
        impl_for_type: None,
    }
}

fn coverage_for_parsed_file<'a>(
    parsed_path: &Path,
    exact: &HashMap<PathBuf, &'a RustLineCoverage>,
    all: &'a [RustLineCoverage],
) -> Option<&'a RustLineCoverage> {
    if let Some(coverage) = exact.get(parsed_path) {
        return Some(*coverage);
    }
    let mut matches = all
        .iter()
        .filter(|coverage| parsed_path.ends_with(&coverage.file));
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

pub fn analysis_from_line_coverage(
    parsed: &[ParsedRustFile],
    line_coverage: &[RustLineCoverage],
) -> RustTestRefAnalysis {
    let mut analysis = empty_analysis();
    let coverage_by_file = line_coverage
        .iter()
        .map(|coverage| (coverage.file.clone(), coverage))
        .collect::<HashMap<_, _>>();
    for file in parsed {
        let Some(file_cov) = coverage_for_parsed_file(&file.path, &coverage_by_file, line_coverage)
        else {
            continue;
        };
        let missing = file_cov
            .missing_lines
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        for line in &file_cov.executable_lines {
            let def = line_definition(file.path.clone(), *line);
            if missing.contains(line) {
                analysis.unreferenced.push(def.clone());
            }
            analysis.definitions.push(def);
        }
    }
    analysis
}

fn fail_closed_analysis(parsed: &[ParsedRustFile]) -> RustTestRefAnalysis {
    let mut analysis = empty_analysis();
    for file in parsed {
        let def = RustCodeDefinition {
            name: "llvm_cov_failed".to_string(),
            kind: CodeUnitKind::Module,
            file: file.path.clone(),
            line: 1,
            impl_for_type: None,
        };
        analysis.definitions.push(def.clone());
        analysis.unreferenced.push(def);
    }
    analysis
}

pub fn runtime_rust_analysis(repo_root: &Path, parsed: &[ParsedRustFile]) -> RustTestRefAnalysis {
    if parsed.is_empty() {
        return empty_analysis();
    }
    if is_nested_cargo_llvm_cov_run() {
        return empty_analysis();
    }
    match collect_cargo_llvm_cov(repo_root) {
        Ok(line_coverage) => analysis_from_line_coverage(parsed, &line_coverage),
        Err(err) => {
            eprintln!("error: cargo llvm-cov coverage failed: {err}");
            fail_closed_analysis(parsed)
        }
    }
}

#[cfg(test)]
#[path = "rust_llvm_cov_path_test.rs"]
mod path_tests;
#[cfg(test)]
#[path = "rust_llvm_cov_test.rs"]
mod tests;
