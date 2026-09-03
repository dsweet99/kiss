pub(super) fn rust_selector_declaration_bytes(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    if rust_signature_ambiguous(&text) {
        return bytes.to_vec();
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(sig) = rust_declaration_signature(trimmed) {
            out.extend_from_slice(sig.as_bytes());
            out.push(b'\n');
        }
    }
    let Ok(file) = syn::parse_file(&text) else {
        return bytes.to_vec();
    };
    append_item_macro_signatures(&file.items, &mut out);
    out
}

fn append_item_macro_signatures(items: &[syn::Item], out: &mut Vec<u8>) {
    for item in items {
        match item {
            syn::Item::Macro(item_macro) => {
                out.extend_from_slice(b"item-macro:");
                for segment in &item_macro.mac.path.segments {
                    out.extend_from_slice(segment.ident.to_string().as_bytes());
                    out.extend_from_slice(b"::");
                }
                out.extend_from_slice(item_macro.mac.tokens.to_string().as_bytes());
                out.push(b'\n');
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    append_item_macro_signatures(nested, out);
                }
            }
            _ => {}
        }
    }
}

fn rust_signature_ambiguous(text: &str) -> bool {
    text.contains("proc_macro") || text.contains("include!") || text.contains("macro_rules!")
}

fn rust_declaration_signature(line: &str) -> Option<&str> {
    if line.starts_with("#[") {
        return Some(line);
    }
    if rust_declaration_line(line) {
        return Some(line.split_once('{').map_or(line, |(sig, _)| sig).trim_end());
    }
    None
}

fn rust_declaration_line(line: &str) -> bool {
    line.starts_with("fn ")
        || line.starts_with("pub fn ")
        || line.starts_with("pub(crate) fn ")
        || line.starts_with("async fn ")
        || line.starts_with("pub async fn ")
        || line.starts_with("mod ")
        || line.starts_with("pub mod ")
}
