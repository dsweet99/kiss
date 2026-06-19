use super::*;
use std::path::{Path, PathBuf};

fn parsed(path: PathBuf) -> ParsedFile {
    let mut parser = crate::parsing::create_parser().unwrap();
    let source = "def fail_closed_sample():\n    return 1\n".to_string();
    let tree = parser.parse(&source, None).unwrap();
    ParsedFile { path, source, tree }
}

#[test]
fn fail_closed_analysis_marks_each_parsed_file_unreferenced() {
    let tmp = tempfile::TempDir::new().unwrap();
    let files = vec![
        parsed(tmp.path().join("a.py")),
        parsed(tmp.path().join("b.py")),
    ];
    let analysis = super::fail_closed_py_analysis(&files);

    assert_eq!(analysis.definitions.len(), 2);
    assert_eq!(analysis.unreferenced.len(), 2);
    assert!(analysis.test_references.is_empty());
    assert!(analysis.call_references.is_empty());
    assert!(analysis.coverage_map.is_empty());
    for (def, file) in analysis.definitions.iter().zip(&files) {
        assert_eq!(def.name, "rslip_refresh_failed");
        assert_eq!(def.kind, CodeUnitKind::Module);
        assert_eq!(def.file, file.path);
        assert_eq!(def.line, 1);
        assert_eq!(def.containing_class, None);
    }
    assert_eq!(analysis.unreferenced.len(), analysis.definitions.len());
    for (missing, def) in analysis.unreferenced.iter().zip(&analysis.definitions) {
        assert_eq!(missing.name, def.name);
        assert_eq!(missing.file, def.file);
        assert_eq!(missing.line, def.line);
    }
}

#[test]
fn runtime_py_analysis_fails_closed_when_refresh_cannot_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("missing-repo");
    let files = vec![parsed(repo.join("pkg/a.py"))];

    let analysis = runtime_py_analysis(&repo, &files, Some(1));

    assert_eq!(analysis.definitions.len(), 1);
    assert_eq!(analysis.unreferenced.len(), 1);
    assert_eq!(analysis.definitions[0].name, "rslip_refresh_failed");
    assert_eq!(analysis.unreferenced[0].file, repo.join("pkg/a.py"));
    assert!(analysis.test_references.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn runtime_py_analysis_fails_closed_for_every_parsed_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = tmp.path().join("missing-repo");
    let files = vec![parsed(repo.join("pkg/a.py")), parsed(repo.join("pkg/b.py"))];

    let analysis = runtime_py_analysis(&repo, &files, Some(1));

    assert_eq!(analysis.definitions.len(), files.len());
    assert_eq!(analysis.unreferenced.len(), files.len());
    for (def, file) in analysis.definitions.iter().zip(&files) {
        assert_eq!(def.name, "rslip_refresh_failed");
        assert_eq!(def.file, file.path);
        assert_eq!(def.line, 1);
    }
    for (missing, file) in analysis.unreferenced.iter().zip(&files) {
        assert_eq!(missing.name, "rslip_refresh_failed");
        assert_eq!(missing.file, file.path);
        assert_eq!(missing.line, 1);
    }
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn fail_closed_analysis_preserves_file_order_and_denied_units() {
    let tmp = tempfile::TempDir::new().unwrap();
    let files = vec![
        parsed(tmp.path().join("pkg/first.py")),
        parsed(tmp.path().join("pkg/second.py")),
        parsed(tmp.path().join("pkg/third.py")),
    ];

    let analysis = fail_closed_py_analysis(&files);

    let definition_files: Vec<_> = analysis
        .definitions
        .iter()
        .map(|def| def.file.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();
    let unreferenced_files: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|def| def.file.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        definition_files,
        vec![
            PathBuf::from("pkg/first.py"),
            PathBuf::from("pkg/second.py"),
            PathBuf::from("pkg/third.py"),
        ]
    );
    assert_eq!(unreferenced_files, definition_files);
    assert!(
        analysis
            .definitions
            .iter()
            .all(|def| def.name == "rslip_refresh_failed" && def.line == 1)
    );
}

#[test]
fn fail_closed_analysis_preserves_duplicate_input_cardinality() {
    let tmp = tempfile::TempDir::new().unwrap();
    let duplicate_path = tmp.path().join("pkg/retried.py");
    let files = vec![
        parsed(duplicate_path.clone()),
        parsed(tmp.path().join("pkg/other.py")),
        parsed(duplicate_path.clone()),
    ];

    let analysis = fail_closed_py_analysis(&files);

    let definition_files: Vec<_> = analysis
        .definitions
        .iter()
        .map(|def| def.file.clone())
        .collect();
    let unreferenced_files: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|def| def.file.clone())
        .collect();
    assert_eq!(
        definition_files,
        vec![
            duplicate_path.clone(),
            tmp.path().join("pkg/other.py"),
            duplicate_path
        ]
    );
    assert_eq!(unreferenced_files, definition_files);
    assert!(
        analysis
            .unreferenced
            .iter()
            .all(|def| def.name == "rslip_refresh_failed" && def.kind == CodeUnitKind::Module)
    );
}

#[test]
fn fail_closed_analysis_keeps_empty_input_empty() {
    let analysis = fail_closed_py_analysis(&[]);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn line_name_and_normalization_are_stable() {
    let root = Path::new("/repo");
    assert_eq!(line_name(17), "line_17");
    assert_eq!(
        normalize_against(root, Path::new("/repo/pkg/a.py")),
        "pkg/a.py"
    );
}
