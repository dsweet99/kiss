use std::path::Path;

pub(crate) fn rust_expanded_include_extras_fingerprint(
    repo: &Path,
    digest_bytes: fn(&[&[u8]]) -> String,
) -> String {
    let root = repo.to_string_lossy().into_owned();
    let (_, discovered) = kiss::gather_files_by_lang_opts(
        std::slice::from_ref(&root),
        Some(kiss::Language::Rust),
        &[],
        false,
    );
    let expanded = kiss::expand_rust_files(discovered.clone());
    let baseline: std::collections::HashSet<_> = discovered.into_iter().collect();
    let mut extras = Vec::new();
    for path in expanded {
        if !baseline.contains(&path) {
            let rel = path
                .strip_prefix(repo)
                .map(|item| item.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            let bytes = std::fs::read(&path).unwrap_or_default();
            extras.push((rel, digest_bytes(&[&bytes])));
        }
    }
    extras.sort();
    let mut parts: Vec<u8> = Vec::new();
    for (rel, digest) in extras {
        parts.extend_from_slice(rel.as_bytes());
        parts.push(0);
        parts.extend_from_slice(digest.as_bytes());
        parts.push(0);
    }
    digest_bytes(&[&parts])
}
