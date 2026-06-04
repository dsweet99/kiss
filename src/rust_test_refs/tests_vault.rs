use super::*;
use crate::rust_parsing::parse_rust_file;

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
