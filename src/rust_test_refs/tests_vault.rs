use super::*;
use crate::rust_graph::build_rust_dependency_graph;
use crate::rust_parsing::parse_rust_file;
use std::path::Path;

fn foil_rust_sources() -> Vec<crate::rust_parsing::ParsedRustFile> {
    let root = Path::new("/tmp/kiss_foil_shpaybxe");
    let mut paths = Vec::new();
    for sub in ["src", "tests"] {
        collect_rs_files(&root.join(sub), &mut paths);
    }
    paths
        .iter()
        .filter_map(|p| parse_rust_file(p).ok())
        .collect()
}

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
}

#[test]
fn test_unreachable_pipeline_callees_not_covered() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault.rs");
    std::fs::write(
        &vault,
        "pub fn stage_0(v: u32) -> u32 { v + 1 }\n\
         pub fn stage_1(v: u32) -> u32 { v + 2 }\n\
         pub fn run_pipeline(seed: u32) -> u32 {\n\
         \x20\x20\x20\x20let mut v = seed;\n\
         \x20\x20\x20\x20v = stage_0(v);\n\
         \x20\x20\x20\x20v = stage_1(v);\n\
         \x20\x20\x20\x20v\n\
         }\n",
    )
    .unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(&lib, "pub mod vault;\n").unwrap();

    let parsed_vault = parse_rust_file(&vault).unwrap();
    let parsed_lib = parse_rust_file(&lib).unwrap();
    let analysis = analyze_rust_test_refs(&[&parsed_vault, &parsed_lib], None);
    let uncovered: Vec<_> = analysis.unreferenced.iter().map(|d| d.name.as_str()).collect();
    assert!(
        uncovered.contains(&"run_pipeline")
            && uncovered.contains(&"stage_0")
            && uncovered.contains(&"stage_1"),
        "pipeline and stages should be unreferenced without tests, got {uncovered:?}"
    );
}

#[test]
fn test_foil_vault_stages_not_covered_full_rust_set() {
    if !Path::new("/tmp/kiss_foil_shpaybxe").exists() {
        return;
    }
    let parsed = foil_rust_sources();
    let refs: Vec<_> = parsed.iter().collect();
    let graph = build_rust_dependency_graph(&refs);
    let analysis = analyze_rust_test_refs(&refs, Some(&graph));
    let _covered_stages: Vec<_> = analysis
        .definitions
        .iter()
        .filter(|d| d.file.to_string_lossy().contains("vault.rs"))
        .filter(|d| d.name.starts_with("stage_"))
        .filter(|d| !analysis.unreferenced.iter().any(|u| u.name == d.name && u.file == d.file))
        .map(|d| d.name.as_str())
        .collect();
    let stage0_uncovered = analysis
        .unreferenced
        .iter()
        .any(|d| d.name == "stage_0" && d.file.to_string_lossy().contains("vault.rs"));
    let run_pipe_uncovered = analysis
        .unreferenced
        .iter()
        .any(|d| d.name == "run_vault_pipeline");
    assert!(
        stage0_uncovered && run_pipe_uncovered,
        "stage_0 uncovered={stage0_uncovered} run_pipeline uncovered={run_pipe_uncovered} \
         stage_0_in_refs={} run_in_refs={} refs={:?}",
        analysis.test_references.contains("stage_0"),
        analysis.test_references.contains("run_vault_pipeline"),
        analysis
            .test_references
            .iter()
            .filter(|r| r.contains("stage") || r.contains("vault") || r.contains("run_vault"))
            .collect::<Vec<_>>()
    );
}
