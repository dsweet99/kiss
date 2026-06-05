use crate::parsing::ParsedFile;

pub fn py_init_marker_pct(parsed: &ParsedFile) -> usize {
    if parsed.path.file_name().and_then(|s| s.to_str()) != Some("__init__.py") {
        return 0;
    }
    let root = parsed.tree.root_node();
    let mut has_import = false;
    super::super::dead_region::collect_py_live_scope(root, &parsed.source, &mut |node| {
        if matches!(node.kind(), "import_from_statement" | "import_statement") {
            has_import = true;
        }
    });
    if has_import { 0 } else { 100 }
}
