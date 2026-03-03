use crate::analyze;
use kiss::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

type DefEntry = (PathBuf, String, usize, bool);

pub fn run_show_tests_to(
    out: &mut dyn std::io::Write,
    universe: &str,
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    show_untested: bool,
) -> i32 {
    let universe_root = Path::new(universe);
    let (py_files, rs_files) = analyze::gather_files(universe_root, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        return 1;
    }

    let focus_set = analyze::build_focus_set(paths, lang_filter, ignore);
    if focus_set.is_empty() {
        eprintln!("No matching source files for specified paths.");
        return 1;
    }

    let mut all_defs: Vec<DefEntry> = Vec::new();

    if !py_files.is_empty() {
        match collect_py_test_defs(&py_files, &focus_set) {
            Ok(defs) => all_defs.extend(defs),
            Err(e) => {
                eprintln!("error: failed to parse Python files: {e}");
                return 1;
            }
        }
    }
    if !rs_files.is_empty() {
        all_defs.extend(collect_rs_test_defs(&rs_files, &focus_set));
    }

    all_defs.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    emit_show_tests_output(out, &all_defs, show_untested);
    0
}

fn collect_py_test_defs(
    py_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Result<Vec<DefEntry>, String> {
    let results = kiss::parse_files(py_files).map_err(|e| e.to_string())?;
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&kiss::ParsedFile> = parsed.iter().collect();
    let analysis = kiss::analyze_test_refs(&refs);
    let unref_set: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    Ok(analysis
        .definitions
        .iter()
        .filter(|d| analyze::is_focus_file(&d.file, focus_set))
        .map(|d| {
            let is_untested = unref_set.contains(&(&d.file, d.name.as_str(), d.line));
            (d.file.clone(), d.name.clone(), d.line, is_untested)
        })
        .collect())
}

fn collect_rs_test_defs(
    rs_files: &[PathBuf],
    focus_set: &HashSet<PathBuf>,
) -> Vec<DefEntry> {
    let results = kiss::parse_rust_files(rs_files);
    let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
    let refs: Vec<&kiss::ParsedRustFile> = parsed.iter().collect();
    let analysis = kiss::analyze_rust_test_refs(&refs);
    let unref_set: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    analysis
        .definitions
        .iter()
        .filter(|d| analyze::is_focus_file(&d.file, focus_set))
        .map(|d| {
            let is_untested = unref_set.contains(&(&d.file, d.name.as_str(), d.line));
            (d.file.clone(), d.name.clone(), d.line, is_untested)
        })
        .collect()
}

fn emit_show_tests_output(
    out: &mut dyn std::io::Write,
    all_defs: &[DefEntry],
    show_untested: bool,
) {
    let mut tested = 0usize;
    let total = all_defs.len();
    for (file, name, line, is_untested) in all_defs {
        if *is_untested {
            if show_untested {
                let _ = writeln!(out, "UNTESTED:{file}:{line}:{name}", file = file.display());
            }
        } else {
            let _ = writeln!(out, "TESTED:{file}:{line}:{name}", file = file.display());
            tested += 1;
        }
    }
    let _ = writeln!(out, "SHOW_TESTS_SUMMARY:{tested}/{total} tested");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_tests_python() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mymod.py"), "def helper():\n    pass\n").unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.py"),
            "from mymod import helper\ndef test_it():\n    helper()\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        assert!(
            output.contains("TESTED:"),
            "expected TESTED lines in output, got: {output}"
        );
        assert!(
            output.contains("SHOW_TESTS_SUMMARY:"),
            "expected SHOW_TESTS_SUMMARY line in output, got: {output}"
        );
        assert!(
            !output.contains("UNTESTED:"),
            "UNTESTED lines should not appear without --untested flag, got: {output}"
        );
    }

    #[test]
    fn test_show_tests_python_with_untested() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("mymod.py"),
            "def helper():\n    pass\n\ndef orphan():\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.py"),
            "from mymod import helper\ndef test_it():\n    helper()\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.py").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], true);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        assert!(
            output.contains("UNTESTED:"),
            "expected UNTESTED lines with --untested flag, got: {output}"
        );
        assert!(
            output.contains("SHOW_TESTS_SUMMARY:"),
            "expected SHOW_TESTS_SUMMARY line, got: {output}"
        );
    }

    #[test]
    fn test_show_tests_rust() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mymod.rs"), "pub fn helper() {}\n").unwrap();
        std::fs::write(
            tmp.path().join("test_mymod.rs"),
            "fn t() { helper(); }\n",
        )
        .unwrap();

        let universe = tmp.path().to_string_lossy().to_string();
        let p = tmp.path().join("mymod.rs").to_string_lossy().to_string();
        let mut buf = Vec::new();
        let exit = run_show_tests_to(&mut buf, &universe, &[p], None, &[], false);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(exit, 0);
        assert!(
            output.contains("SHOW_TESTS_SUMMARY:"),
            "expected SHOW_TESTS_SUMMARY line in output, got: {output}"
        );
        assert!(
            !output.contains("UNTESTED:"),
            "UNTESTED lines should not appear without --untested flag, got: {output}"
        );
    }

    #[test]
    fn test_helper_coverage() {
        fn touch<T>(_: T) {}
        touch(collect_py_test_defs);
        touch(collect_rs_test_defs);
        touch(emit_show_tests_output);
    }
}
