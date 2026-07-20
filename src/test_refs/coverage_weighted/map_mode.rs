use crate::parsing::ParsedFile;

pub fn py_init_marker_pct(parsed: &ParsedFile) -> usize {
    if parsed.path.file_name().and_then(|s| s.to_str()) != Some("__init__.py") {
        return 0;
    }
    let root = parsed.tree.root_node();
    let mut has_import = false;
    super::super::scope::collect_py_scope(root, &parsed.source, &mut |node| {
        if matches!(node.kind(), "import_from_statement" | "import_statement") {
            has_import = true;
        }
    });
    if has_import { 0 } else { 100 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};

    #[test]
    fn py_init_marker_pct_detects_import_statement_in_init_file() {
        let tmp = tempfile::tempdir().unwrap();
        let init = tmp.path().join("__init__.py");
        std::fs::write(&init, "import os\n").unwrap();
        let mut parser = create_parser().unwrap();
        let parsed = parse_file(&mut parser, &init).unwrap();

        assert_eq!(py_init_marker_pct(&parsed), 0);
    }
}
