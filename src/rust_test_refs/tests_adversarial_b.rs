use crate::rust_parsing::parse_rust_file;
use crate::units::CodeUnitKind;
use std::collections::HashSet;
use std::path::Path;

/// principles.md: Development methodology §4 — structural mechanisms over thresholds; Formal model §3 — bounded propagation
/// smell_registry: numeric_staircase
/// metamorphic: decoy_injection_monotonicity
/// follow_up: inject k filler defs (k swept 0..=16) into import-cal neighbor module
/// invariant: import-cal credit for a neighbor helper must not flip uncovered solely because decoy defs push module past MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE
/// counterexample: witness.rs → neighbor.rs one-hop import; seed witnessed; neighbor holds target helper plus k decoy fns
#[test]
fn import_cal_neighbor_credit_must_be_monotone_under_decoy_injection() {
    use super::calibration::apply_rust_import_dependency_calibration;
    use crate::graph::DependencyGraph;

    fn neighbor_helper_covered(decoy_count: usize) -> bool {
        let tmp = tempfile::TempDir::new().unwrap();
        let witness_path = tmp.path().join("src/acme/witness.rs");
        let neighbor_path = tmp.path().join("src/acme/neighbor.rs");
        std::fs::create_dir_all(witness_path.parent().unwrap()).unwrap();
        std::fs::write(&witness_path, "pub fn seed() {}\n").unwrap();
        let mut neighbor_src = String::from("pub fn target() {}\n");
        for i in 0..decoy_count {
            neighbor_src.push_str(&format!("pub fn decoy_{i}() {{}}\n"));
        }
        std::fs::write(&neighbor_path, neighbor_src).unwrap();

        let mut graph = DependencyGraph::new();
        let witness_canon = crate::rust_include::canonical_path(&witness_path);
        let neighbor_canon = crate::rust_include::canonical_path(&neighbor_path);
        graph
            .path_to_module
            .insert(witness_canon, "acme_witness".into());
        graph
            .path_to_module
            .insert(neighbor_canon, "acme_neighbor".into());
        graph.get_or_create_node("acme_witness");
        graph.get_or_create_node("acme_neighbor");
        graph.add_dependency("acme_witness", "acme_neighbor");

        let mut definitions = vec![super::RustCodeDefinition {
            name: "seed".into(),
            kind: CodeUnitKind::Function,
            file: witness_path,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        }];
        let mut line = 1usize;
        definitions.push(super::RustCodeDefinition {
            name: "target".into(),
            kind: CodeUnitKind::Function,
            file: neighbor_path.clone(),
            line,
            end_line: line,
            impl_for_type: None,
        });
        line += 1;
        for i in 0..decoy_count {
            definitions.push(super::RustCodeDefinition {
                name: format!("decoy_{i}"),
                kind: CodeUnitKind::Function,
                file: neighbor_path.clone(),
                line,
                end_line: line,
                impl_for_type: None,
            });
            line += 1;
        }

        let witness = HashSet::from(["seed".to_string()]);
        let mut unreferenced: Vec<super::RustCodeDefinition> = definitions
            .iter()
            .filter(|d| d.name != "seed")
            .cloned()
            .collect();
        apply_rust_import_dependency_calibration(
            &definitions,
            &mut unreferenced,
            &graph,
            &witness,
        );
        !unreferenced.iter().any(|d| d.name == "target")
    }

    let baseline_covered = neighbor_helper_covered(0);
    assert!(
        baseline_covered,
        "neighbor helper must be import-cal credited with zero decoys when seed is witnessed"
    );

    for k in 1..=16 {
        let covered = neighbor_helper_covered(k);
        assert!(
            covered,
            "import-cal credit must stay monotone: decoy_count={k} revoked credit that held at 0"
        );
    }
}

fn parse_all_rs_under(root: &Path) -> Vec<super::ParsedRustFile> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(parse_rust_file(&path).unwrap());
            }
        }
    }
    files
}

/// principles.md: Pathology handling §2 — evidence channels stay separate; §4 fail closed
/// smell_registry: channel_collapse
/// metamorphic: subprocess_partition_isolation
/// follow_up: add tests/unit.rs with direct handler() call witness
/// invariant: subprocess-only repos must not credit production defs witnessed only via excluded subprocess partition
/// counterexample: lone tests/spawn.rs (Command::new) + src/main.rs→handler cone, zero non-subprocess test files
#[test]
fn subprocess_only_repo_must_not_hollow_credit_production_via_excluded_witnesses() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    std::fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() { handler(); }\npub fn handler() {}\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("tests/spawn.rs"),
        "#[test]\nfn integration() {\n    let _ = std::process::Command::new(\"app\");\n    handler();\n}\n",
    )
    .unwrap();

    let parsed = parse_all_rs_under(tmp.path());
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = super::analyze_rust_test_refs_for_coverage_map(&refs, None);

    assert!(
        analysis
            .unreferenced
            .iter()
            .any(|d| d.name == "handler"),
        "subprocess-only repo must fail closed: handler credited via excluded witness partition"
    );
    assert!(
        analysis
            .unreferenced
            .iter()
            .any(|d| d.name == "main"),
        "subprocess-only repo must fail closed: main credited via binary-cone channel collapse"
    );
}
