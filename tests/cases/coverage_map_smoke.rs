use std::process::Command;

#[test]
fn kiss_coverage_map_emits_json_for_fixture() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let fixture = format!("{manifest}/tests/fake_python");
    let bin = format!("{manifest}/target/debug/kiss-coverage-map");
    let output = Command::new(&bin)
        .arg(&fixture)
        .output()
        .unwrap_or_else(|e| panic!("run {bin}: {e}"));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("coverage map json");
    assert!(parsed.is_object());
}
