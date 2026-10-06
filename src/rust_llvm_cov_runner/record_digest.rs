use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syn::spanned::Spanned;
use syn::visit::Visit;

fn skeleton(text: &str) -> Option<(String, Vec<(usize, usize)>)> {
    let file = syn::parse_file(text).ok()?;
    let mut bodies = FnBodies::default();
    bodies.visit_file(&file);
    let lines: Vec<&str> = text.lines().collect();
    let in_body = line_mask(lines.len(), bodies.ranges.iter().copied());
    let in_body = |line: usize| in_body[line];
    let skeleton = (1..=lines.len())
        .filter(|&line| !in_body(line))
        .map(|line| lines[line - 1])
        .filter(|line| !line.trim().is_empty());
    Some((digest_lines(b"skeleton", skeleton), bodies.ranges))
}

fn line_mask(len: usize, ranges: impl Iterator<Item = (usize, usize)>) -> Vec<bool> {
    let mut mask = vec![false; len + 1];
    for (a, b) in ranges {
        for slot in mask.iter_mut().take(b.min(len) + 1).skip(a) {
            *slot = true;
        }
    }
    mask
}

#[cfg(test)]
pub(crate) fn covered_items_digest(text: &str, covered: &BTreeSet<u32>) -> String {
    ParsedItems::parse(text.to_string()).covered_digest(covered)
}

pub(crate) struct ParsedItems {
    text: String,
    skeleton: Option<(String, Vec<(usize, usize)>)>,
}

impl ParsedItems {
    pub(crate) fn parse(text: String) -> Self {
        let skeleton = skeleton(&text);
        Self { text, skeleton }
    }

    pub(crate) fn covered_digest(&self, covered: &BTreeSet<u32>) -> String {
        let Some((skeleton, ranges)) = &self.skeleton else {
            return digest_lines(b"unparsed", self.text.lines());
        };
        let lines: Vec<&str> = self.text.lines().collect();
        let mut h = fold(OFFSET, skeleton.as_bytes());
        let mut selected: Vec<(usize, usize)> = ranges
            .iter()
            .copied()
            .filter(|&(a, b)| {
                covered
                    .iter()
                    .any(|&line| a <= line as usize && line as usize <= b)
            })
            .collect();
        selected.sort_unstable();
        selected.dedup();
        for (a, b) in selected {
            let body = lines.get(a - 1..b.min(lines.len())).unwrap_or_default();
            h = fold(h, &digest_lines(b"body", body.iter().copied()).into_bytes());
        }
        format!("{h:016x}")
    }
}

pub(crate) fn test_definition_digests(rel: &str, text: &str) -> BTreeMap<String, String> {
    let Ok(file) = syn::parse_file(text) else {
        return BTreeMap::new();
    };
    let mut tests = TestFns::default();
    tests.visit_file(&file);
    let lines: Vec<&str> = text.lines().collect();
    let in_test = line_mask(lines.len(), tests.found.iter().map(|&(_, a, b)| (a, b)));
    let in_test = |line: usize| in_test[line];
    let support = (1..=lines.len())
        .filter(|&line| !in_test(line))
        .map(|line| lines[line - 1])
        .filter(|line| !line.trim().is_empty());
    let mut digests = BTreeMap::from([(test_support_key(rel), digest_lines(b"support", support))]);
    digests.extend(tests.found.iter().map(|(name, a, b)| {
        let body = lines.get(a - 1..(*b).min(lines.len())).unwrap_or_default();
        (
            format!("{rel}::{name}"),
            digest_lines(b"test", body.iter().copied()),
        )
    }));
    digests
}

pub(crate) fn is_coverage_excluded(rel: &str) -> bool {
    let mut parts: Vec<&str> = rel.split('/').collect();
    let Some(name) = parts.pop() else {
        return false;
    };
    parts
        .iter()
        .any(|dir| matches!(*dir, "tests" | "examples" | "benches"))
        || name == "tests.rs"
        || name.strip_suffix("tests.rs").is_some_and(|stem| {
            stem.len() > 1
                && (stem.ends_with('_') || stem.ends_with('-'))
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

pub(crate) fn compile_surface_digest(root: &Path, rels: &[String]) -> String {
    use rayon::prelude::*;
    let surfaces: Vec<(&str, String)> = rels
        .par_iter()
        .filter(|rel| rel.ends_with(".rs") || rel.ends_with(".inc"))
        .map(|rel| (rel.as_str(), file_compile_surface(root, rel)))
        .collect();
    let h = surfaces
        .iter()
        .fold(fold(OFFSET, b"compile-surface"), |h, (rel, surface)| {
            fold(fold(fold(h, rel.as_bytes()), b"\0"), surface.as_bytes())
        });
    format!("{h:016x}")
}

fn file_compile_surface(root: &Path, rel: &str) -> String {
    let text = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
    let Ok(tokens) = text.parse::<proc_macro2::TokenStream>() else {
        return digest_lines(b"unparsed", text.lines());
    };
    let surface = Surface {
        tests_only: rel.ends_with(".rs") && is_coverage_excluded(rel),
    };
    format!("{:016x}", surface.scan(tokens))
}

struct Surface {
    tests_only: bool,
}

struct ItemState {
    h: u64,
    item: u64,
    after_fn: bool,
    is_const: bool,
    is_test: bool,
    in_attr: bool,
}

impl ItemState {
    fn new() -> Self {
        Self {
            h: fold(OFFSET, b"surface"),
            item: OFFSET,
            after_fn: false,
            is_const: false,
            is_test: false,
            in_attr: false,
        }
    }

    fn end_item(&mut self) {
        if !(self.is_test && self.after_fn) {
            self.h = fold(self.h, &self.item.to_le_bytes());
        }
        *self = Self {
            h: self.h,
            ..Self::new()
        };
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.item = fold(fold(self.item, bytes), b" ");
    }
}

impl Surface {
    fn scan(&self, tokens: proc_macro2::TokenStream) -> u64 {
        use proc_macro2::TokenTree;
        let mut state = ItemState::new();
        for token in tokens {
            match token {
                TokenTree::Group(group) => self.group(&group, &mut state),
                TokenTree::Ident(ident) => Self::ident(&ident.to_string(), &mut state),
                TokenTree::Punct(punct) => Self::punct(punct.as_char(), &mut state),
                TokenTree::Literal(literal) => {
                    state.feed(literal.to_string().as_bytes());
                    state.in_attr = false;
                }
            }
        }
        state.end_item();
        state.h
    }

    fn group(&self, group: &proc_macro2::Group, state: &mut ItemState) {
        use proc_macro2::{Delimiter, TokenTree};
        let brace = group.delimiter() == Delimiter::Brace;
        if state.in_attr && group.delimiter() == Delimiter::Bracket {
            state.is_test |= group
                .stream()
                .into_iter()
                .any(|t| matches!(t, TokenTree::Ident(ref id) if id == "test"));
        }
        let body = brace && state.after_fn && !state.is_const;
        if body && !self.tests_only {
            state.feed(b"{fn}");
        } else {
            let inner = self.scan(group.stream());
            state.feed(&[group.delimiter() as u8]);
            state.feed(&inner.to_le_bytes());
        }
        state.in_attr = false;
        if brace {
            state.end_item();
        }
    }

    fn ident(text: &str, state: &mut ItemState) {
        match text {
            "fn" => state.after_fn = true,
            "const" => state.is_const = !state.after_fn,
            _ => {}
        }
        state.feed(text.as_bytes());
        state.in_attr = false;
    }

    fn punct(ch: char, state: &mut ItemState) {
        state.feed(ch.to_string().as_bytes());
        state.in_attr = ch == '#';
        if ch == ';' {
            state.end_item();
        }
    }
}

pub(crate) fn test_support_key(rel: &str) -> String {
    format!("{rel}::")
}

pub(crate) fn selector_leaf(selector: &str) -> &str {
    selector.rsplit(['$', ':']).next().unwrap_or(selector)
}

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fold(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |acc, byte| {
        (acc ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn digest_lines<'a>(tag: &[u8], lines: impl Iterator<Item = &'a str>) -> String {
    let h = lines.fold(fold(OFFSET, tag), |h, line| {
        fold(fold(h, line.as_bytes()), b"\n")
    });
    format!("{h:016x}")
}

#[derive(Default)]
struct FnBodies {
    ranges: Vec<(usize, usize)>,
}

impl FnBodies {
    fn push(&mut self, block: &syn::Block) {
        let span = block.span();
        self.ranges.push((span.start().line, span.end().line));
    }
}

impl<'ast> Visit<'ast> for FnBodies {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if item.sig.constness.is_none() {
            self.push(&item.block);
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if item.sig.constness.is_none() {
            self.push(&item.block);
        }
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(block) = &item.default {
            self.push(block);
        }
        syn::visit::visit_trait_item_fn(self, item);
    }
}

#[derive(Default)]
struct TestFns {
    found: Vec<(String, usize, usize)>,
}

impl<'ast> Visit<'ast> for TestFns {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let is_test = item.attrs.iter().any(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "test")
        });
        if is_test {
            let span = item.span();
            self.found.push((
                item.sig.ident.to_string(),
                span.start().line,
                span.end().line,
            ));
        }
        syn::visit::visit_item_fn(self, item);
    }
}

#[cfg(test)]
#[path = "record_digest_test.rs"]
mod record_digest_test;
