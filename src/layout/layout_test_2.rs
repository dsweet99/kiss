use super::*;

#[test]
fn test_count_layering_violations_with_manual_layers() {
    use kiss::LayerInfo;

    let mut graph = DependencyGraph::new();
    graph.add_dependency("foundation", "app");

    let layer_info = LayerInfo {
        layers: vec![vec!["foundation".to_string()], vec!["app".to_string()]],
    };

    assert_eq!(count_layering_violations(&graph, &layer_info), 1);
}

#[test]
fn test_count_layering_violations_missing_layer_info() {
    use kiss::LayerInfo;

    let mut graph = DependencyGraph::new();
    graph.add_dependency("a", "unknown");

    let layer_info = LayerInfo {
        layers: vec![vec!["a".to_string()]],
    };

    assert_eq!(count_layering_violations(&graph, &layer_info), 0);
}

#[test]
fn test_layout_options_struct_fields() {
    let paths = vec!["src".to_string()];
    let ignore_prefixes = vec!["test_".to_string()];

    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: Some(Language::Python),
        ignore_prefixes: &ignore_prefixes,
        project_name: Some("my_project".to_string()),
    };

    assert_eq!(opts.paths, &["src".to_string()]);
    assert_eq!(opts.lang_filter, Some(Language::Python));
    assert_eq!(opts.ignore_prefixes, &["test_".to_string()]);
    assert_eq!(opts.project_name, Some("my_project".to_string()));
}

#[test]
fn test_analyze_layout_with_python_files() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let py_file = temp_dir.path().join("module_a.py");
    std::fs::write(&py_file, "import module_b\n").unwrap();

    let py_file_b = temp_dir.path().join("module_b.py");
    std::fs::write(&py_file_b, "# no imports\n").unwrap();

    let py_files = vec![py_file, py_file_b];
    let rs_files: Vec<PathBuf> = vec![];

    let paths: Vec<String> = vec![];
    let ignore_prefixes: Vec<String> = vec![];
    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: None,
        ignore_prefixes: &ignore_prefixes,
        project_name: Some("test_project".to_string()),
    };

    let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
    assert_eq!(analysis.project_name, "test_project");

    assert!(analysis.layer_info.num_layers() > 0);
}

#[test]
fn test_analyze_layout_with_rust_files() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let rs_file = temp_dir.path().join("lib.rs");
    std::fs::write(&rs_file, "mod utils;\nfn main() {}\n").unwrap();

    let rs_file_b = temp_dir.path().join("utils.rs");
    std::fs::write(&rs_file_b, "pub fn helper() {}\n").unwrap();

    let py_files: Vec<PathBuf> = vec![];
    let rs_files = vec![rs_file, rs_file_b];

    let paths: Vec<String> = vec![];
    let ignore_prefixes: Vec<String> = vec![];
    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: None,
        ignore_prefixes: &ignore_prefixes,
        project_name: Some("rust_project".to_string()),
    };

    let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
    assert_eq!(analysis.project_name, "rust_project");
}

#[test]
fn test_analyze_layout_project_name_custom() {
    let py_files: Vec<PathBuf> = vec![];
    let rs_files: Vec<PathBuf> = vec![];
    let paths: Vec<String> = vec![];
    let ignore_prefixes: Vec<String> = vec![];

    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: None,
        ignore_prefixes: &ignore_prefixes,
        project_name: Some("custom_name".to_string()),
    };

    let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();
    assert_eq!(analysis.project_name, "custom_name");
}

#[test]
fn test_analyze_layout_project_name_default() {
    let py_files: Vec<PathBuf> = vec![];
    let rs_files: Vec<PathBuf> = vec![];
    let paths: Vec<String> = vec![];
    let ignore_prefixes: Vec<String> = vec![];

    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: None,
        ignore_prefixes: &ignore_prefixes,
        project_name: None,
    };

    let analysis = analyze_layout(&py_files, &rs_files, &opts).unwrap();

    assert!(!analysis.project_name.is_empty());
}

#[test]
fn test_run_layout_to_file() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let py_file = temp_dir.path().join("app.py");
    std::fs::write(&py_file, "# simple module\n").unwrap();

    let out_file = temp_dir.path().join("layout_output.md");
    let path_str = temp_dir.path().to_string_lossy().to_string();

    let paths = vec![path_str];
    let ignore_prefixes: Vec<String> = vec![];
    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: Some(Language::Python),
        ignore_prefixes: &ignore_prefixes,
        project_name: Some("file_test".to_string()),
    };

    run_layout(&opts, Some(&out_file)).unwrap();

    assert!(out_file.exists());
    let content = std::fs::read_to_string(&out_file).unwrap();
    assert!(content.contains("file_test") || !content.is_empty());
}

#[test]
fn test_run_layout_no_files_error() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let path_str = temp_dir.path().to_string_lossy().to_string();

    let paths = vec![path_str];
    let ignore_prefixes: Vec<String> = vec![];
    let opts = LayoutOptions {
        paths: &paths,
        lang_filter: None,
        ignore_prefixes: &ignore_prefixes,
        project_name: None,
    };

    let result = run_layout(&opts, None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("No source files"));
}
#[test]
fn test_project_name_from_paths() {
    let paths = vec!["/tmp/myproject/src".to_string()];

    let _ = project_name_from_paths(&paths);

    assert!(project_name_from_paths(&[]).is_none());
}

#[test]
fn test_project_name_from_cwd() {
    let result = project_name_from_cwd();
    assert!(result.is_some());
}
