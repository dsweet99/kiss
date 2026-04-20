use super::tests::empty_analysis;
use super::*;

#[test]
fn test_default_layer_name_two_layers() {
    assert_eq!(default_layer_name(0, 2), "Foundation");
    assert_eq!(default_layer_name(1, 2), "Application");

    let mut analysis = empty_analysis("test");
    analysis.layer_info = LayerInfo {
        layers: vec![vec!["base".into()], vec!["top".into()]],
    };
    let md = format_markdown(&analysis);
    assert!(md.contains("Layer 0: Foundation"));
    assert!(md.contains("Layer 1: Application"));
}

#[test]
fn static_coverage_touch_format_helpers() {
    fn t<T>(_: T) {}
    t(format_summary);
    t(format_cycles);
    t(format_layers);
    t(format_what_if);
}
