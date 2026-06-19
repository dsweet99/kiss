use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use crate::coverage::{executable_lines_from_source, line_coverage};
use crate::database::{load_database, write_database_atomic};
use crate::discovery::{config_fingerprints, discover_repo_files};
use crate::refresh::{changed_files, coverage_refresh_pytest_extra};
use crate::types::{CoverageMetadata, Database, FileRecord, FileRole};
use crate::util::normalize_path;
use crate::{RSLIP_VERSION, SCHEMA_VERSION};

#[derive(serde::Deserialize)]
struct SlipcoverPayload {
    files: BTreeMap<String, SlipcoverFile>,
}

#[derive(serde::Deserialize)]
struct SlipcoverFile {
    executed_lines: Vec<usize>,
    missing_lines: Vec<usize>,
}

pub fn refresh_line_coverage_and_store(repo_root: &Path, j: usize) -> Result<Database, String> {
    if let Some(db) = current_line_coverage_database(repo_root)? {
        return Ok(db);
    }
    let db = refresh_line_coverage(repo_root, j)?;
    write_database_atomic(repo_root, &db)?;
    Ok(db)
}

fn current_line_coverage_database(repo_root: &Path) -> Result<Option<Database>, String> {
    let Some(db) = load_database(repo_root)? else {
        return Ok(None);
    };
    if !db.tests.is_empty() || !db.source_to_covering_tests.is_empty() {
        return Ok(None);
    }
    if changed_files(repo_root, &db)?.is_empty() {
        return Ok(Some(db));
    }
    Ok(None)
}

pub fn refresh_line_coverage(repo_root: &Path, _j: usize) -> Result<Database, String> {
    let mut files = discover_repo_files(repo_root)?;
    let extra = coverage_refresh_pytest_extra(repo_root);
    let coverage = run_slipcover_line_coverage(repo_root, &extra)?;
    apply_slipcover_coverage(repo_root, &mut files, &coverage);
    let file_map = files
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect();
    Ok(Database {
        schema_version: SCHEMA_VERSION,
        rslip_version: RSLIP_VERSION.to_string(),
        config_fingerprints: config_fingerprints(&files),
        files: file_map,
        tests: BTreeMap::new(),
        source_to_covering_tests: BTreeMap::new(),
    })
}

fn run_slipcover_line_coverage(
    repo_root: &Path,
    pytest_extra: &[String],
) -> Result<BTreeMap<String, CoverageMetadata>, String> {
    run_slipcover_line_coverage_with_program(repo_root, pytest_extra, Path::new("slipcover"))
}

fn run_slipcover_line_coverage_with_program(
    repo_root: &Path,
    pytest_extra: &[String],
    slipcover_program: &Path,
) -> Result<BTreeMap<String, CoverageMetadata>, String> {
    let out_path = std::env::temp_dir().join(format!(
        "kiss-rslip-slipcover-{}-{}.json",
        std::process::id(),
        repo_root.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut cmd = Command::new(slipcover_program);
    cmd.arg("--json")
        .arg("--source")
        .arg(repo_root)
        .arg("--out")
        .arg(&out_path)
        .arg("-m")
        .arg("pytest");
    if pytest_extra.is_empty() {
        cmd.arg(repo_root);
    } else {
        cmd.args(pytest_extra);
    }
    cmd.current_dir(repo_root);
    let output = output_with_text_file_busy_retry(&mut cmd)
        .map_err(|e| format!("failed to run slipcover: {e}"))?;
    if !out_path.is_file() {
        return Err(format!(
            "slipcover did not write coverage output (exit {:?})\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let bytes = fs::read(&out_path)
        .map_err(|e| format!("read slipcover output {}: {e}", out_path.display()))?;
    let _ = fs::remove_file(&out_path);
    let payload: SlipcoverPayload =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse slipcover output: {e}"))?;
    let mut coverage = BTreeMap::new();
    for (path, file) in payload.files {
        coverage.insert(
            normalize_path(repo_root, Path::new(&path)),
            coverage_from_slipcover(file),
        );
    }
    if !output.status.success() {
        return Err(format!(
            "slipcover failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(coverage)
}

fn output_with_text_file_busy_retry(cmd: &mut Command) -> std::io::Result<Output> {
    const TEXT_FILE_BUSY_OS_ERROR: i32 = 26;
    const MAX_ATTEMPTS: usize = 5;

    let mut delay = Duration::from_millis(10);
    for attempt in 1..=MAX_ATTEMPTS {
        match cmd.output() {
            Ok(output) => return Ok(output),
            Err(err)
                if err.raw_os_error() == Some(TEXT_FILE_BUSY_OS_ERROR)
                    && attempt < MAX_ATTEMPTS =>
            {
                std::thread::sleep(delay);
                delay = delay.saturating_mul(2);
            }
            Err(err) => return Err(err),
        }
    }
    unreachable!("loop returns on the last attempt");
}

fn coverage_from_slipcover(file: SlipcoverFile) -> CoverageMetadata {
    let executed: BTreeSet<_> = file.executed_lines.into_iter().collect();
    let missing: BTreeSet<_> = file.missing_lines.into_iter().collect();
    let executable: BTreeSet<_> = executed.union(&missing).copied().collect();
    let executed_lines: Vec<_> = executed.into_iter().collect();
    let missing_lines: Vec<_> = missing.into_iter().collect();
    let percent_covered = if executable.is_empty() {
        100
    } else {
        ((executed_lines.len() * 100) + (executable.len() / 2)) / executable.len()
    };
    CoverageMetadata {
        executable_lines: executable.into_iter().collect(),
        executed_lines,
        missing_lines,
        percent_covered,
    }
}

fn apply_slipcover_coverage(
    repo_root: &Path,
    files: &mut [FileRecord],
    coverage: &BTreeMap<String, CoverageMetadata>,
) {
    for file in files {
        if file.role == FileRole::Source {
            file.coverage = coverage.get(&file.path).cloned().or_else(|| {
                let source = fs::read_to_string(repo_root.join(&file.path)).unwrap_or_default();
                Some(line_coverage(
                    &executable_lines_from_source(&source),
                    &BTreeSet::new(),
                ))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn coverage_from_slipcover_handles_empty_line_sets() {
        let coverage = coverage_from_slipcover(SlipcoverFile {
            executed_lines: Vec::new(),
            missing_lines: Vec::new(),
        });

        assert_eq!(coverage.percent_covered, 100);
        assert!(coverage.executable_lines.is_empty());
        assert!(coverage.executed_lines.is_empty());
        assert!(coverage.missing_lines.is_empty());
    }

    #[test]
    fn coverage_from_slipcover_normalizes_line_sets_and_rounds_percent() {
        let coverage = coverage_from_slipcover(SlipcoverFile {
            executed_lines: vec![5, 1, 1],
            missing_lines: vec![3, 2, 3],
        });

        assert_eq!(coverage.executable_lines, vec![1, 2, 3, 5]);
        assert_eq!(coverage.executed_lines, vec![1, 5]);
        assert_eq!(coverage.missing_lines, vec![2, 3]);
        assert_eq!(coverage.percent_covered, 50);
    }

    #[test]
    fn slipcover_payload_deserializes_file_map() {
        let payload: SlipcoverPayload = serde_json::from_str(
            r#"{"files":{"pkg.py":{"executed_lines":[2,1],"missing_lines":[3]}}}"#,
        )
        .unwrap();
        let payload = SlipcoverPayload {
            files: payload.files,
        };

        let file = payload.files.get("pkg.py").unwrap();
        assert_eq!(file.executed_lines, vec![2, 1]);
        assert_eq!(file.missing_lines, vec![3]);
    }

    #[test]
    fn current_line_coverage_database_reuses_fresh_line_refresh_database() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pkg.py"), "value = 1\n").unwrap();
        let records = discover_repo_files(tmp.path()).unwrap();
        let files = records
            .iter()
            .map(|file| (file.path.clone(), file.clone()))
            .collect();
        let cached = Database {
            schema_version: SCHEMA_VERSION,
            rslip_version: RSLIP_VERSION.to_string(),
            config_fingerprints: config_fingerprints(&records),
            files,
            tests: BTreeMap::new(),
            source_to_covering_tests: BTreeMap::new(),
        };
        write_database_atomic(tmp.path(), &cached).unwrap();

        let loaded = current_line_coverage_database(tmp.path())
            .unwrap()
            .expect("fresh line-refresh database should be reused");

        assert_eq!(loaded, cached);
    }

    #[test]
    fn current_line_coverage_database_rejects_per_test_database() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pkg.py"), "value = 1\n").unwrap();
        let records = discover_repo_files(tmp.path()).unwrap();
        let files = records
            .iter()
            .map(|file| (file.path.clone(), file.clone()))
            .collect();
        let cached = Database {
            schema_version: SCHEMA_VERSION,
            rslip_version: RSLIP_VERSION.to_string(),
            config_fingerprints: config_fingerprints(&records),
            files,
            tests: BTreeMap::from([(
                "test_pkg.py::test_value".to_string(),
                crate::types::TestRecord {
                    selector: "test_pkg.py::test_value".to_string(),
                    test_path: "test_pkg.py".to_string(),
                    content_digest: "test-digest".to_string(),
                    covered_files: vec!["pkg.py".to_string()],
                    covered_lines: BTreeMap::new(),
                },
            )]),
            source_to_covering_tests: BTreeMap::new(),
        };
        write_database_atomic(tmp.path(), &cached).unwrap();

        assert!(
            current_line_coverage_database(tmp.path())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn run_slipcover_line_coverage_defaults_to_repo_root_pytest_target() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pkg.py"), "def value():\n    return 1\n").unwrap();
        fs::write(
            tmp.path().join("test_pkg.py"),
            "from pkg import value\n\ndef test_value():\n    assert value() == 1\n",
        )
        .unwrap();

        let coverage = run_slipcover_line_coverage(tmp.path(), &[]).unwrap();

        assert_eq!(coverage["pkg.py"].executed_lines, vec![1, 2]);
        assert_eq!(coverage["pkg.py"].missing_lines, Vec::<usize>::new());
    }

    #[test]
    fn run_slipcover_line_coverage_rejects_partial_coverage_on_pytest_failure() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pkg.py"), "value = 1\n").unwrap();

        let err =
            run_slipcover_line_coverage(tmp.path(), &["missing_target".to_string()]).unwrap_err();

        assert!(err.contains("slipcover failed"), "{err}");
    }

    #[test]
    fn run_slipcover_line_coverage_reports_missing_output() {
        let tmp = TempDir::new().unwrap();
        let fake_slipcover = tmp.path().join("fake-slipcover-no-output");
        write_executable(
            &fake_slipcover,
            "#!/bin/sh\necho fake stdout\necho fake stderr >&2\nexit 0\n",
        );

        let err =
            run_slipcover_line_coverage_with_program(tmp.path(), &[], &fake_slipcover).unwrap_err();

        assert!(err.contains("slipcover did not write coverage output"));
        assert!(err.contains("stdout:\nfake stdout"));
        assert!(err.contains("stderr:\nfake stderr"));
    }

    #[test]
    fn run_slipcover_line_coverage_rejects_empty_coverage_after_failure() {
        let tmp = TempDir::new().unwrap();
        let fake_slipcover = tmp.path().join("fake-slipcover-empty-failure");
        write_executable(
            &fake_slipcover,
            r#"#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--out" ]; then
        shift
        out="$1"
    fi
    shift || true
done
printf '{"files":{}}\n' > "$out"
echo fake stdout
echo fake stderr >&2
exit 7
"#,
        );
        let err =
            run_slipcover_line_coverage_with_program(tmp.path(), &[], &fake_slipcover).unwrap_err();

        assert!(err.contains("slipcover failed (exit Some(7))"), "{err}");
        assert!(err.contains("stdout:\nfake stdout"));
        assert!(err.contains("stderr:\nfake stderr"));
    }

    #[test]
    fn run_slipcover_line_coverage_rejects_nonempty_coverage_after_failure() {
        let tmp = TempDir::new().unwrap();
        let fake_slipcover = tmp.path().join("fake-slipcover-nonempty-failure");
        write_executable(
            &fake_slipcover,
            r#"#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--out" ]; then
        shift
        out="$1"
    fi
    shift || true
done
printf '{"files":{"pkg.py":{"executed_lines":[1],"missing_lines":[2]}}}\n' > "$out"
exit 7
"#,
        );

        let err =
            run_slipcover_line_coverage_with_program(tmp.path(), &[], &fake_slipcover).unwrap_err();

        assert!(err.contains("slipcover failed (exit Some(7))"), "{err}");
    }

    #[test]
    fn apply_slipcover_coverage_falls_back_to_static_missing_lines() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pkg.py"), "def cold():\n    return 1\n").unwrap();
        fs::write(
            tmp.path().join("test_pkg.py"),
            "def test_pkg():\n    assert 1\n",
        )
        .unwrap();
        let mut files = discover_repo_files(tmp.path()).unwrap();

        apply_slipcover_coverage(tmp.path(), &mut files, &BTreeMap::new());

        let pkg = files.iter().find(|file| file.path == "pkg.py").unwrap();
        let coverage = pkg.coverage.as_ref().unwrap();
        assert_eq!(coverage.executable_lines, vec![1, 2]);
        assert_eq!(coverage.executed_lines, Vec::<usize>::new());
        assert_eq!(coverage.missing_lines, vec![1, 2]);
        assert_eq!(coverage.percent_covered, 0);
        let test = files
            .iter()
            .find(|file| file.path == "test_pkg.py")
            .unwrap();
        assert_eq!(test.coverage, None);
    }
}
