use syn::spanned::Spanned;

pub(super) fn help_doc_ranges(ast: &syn::File) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    collect_from_items(&ast.items, &mut ranges);
    ranges
}

pub(super) fn is_help_doc(ranges: &[(usize, usize)], byte_idx: usize) -> bool {
    ranges
        .iter()
        .any(|(start, end)| byte_idx >= *start && byte_idx < *end)
}

fn collect_from_items(items: &[syn::Item], ranges: &mut Vec<(usize, usize)>) {
    for item in items {
        match item {
            syn::Item::Struct(item) => collect_struct(item, ranges),
            syn::Item::Enum(item) => collect_enum(item, ranges),
            syn::Item::Mod(item) => {
                if let Some((_, nested)) = &item.content {
                    collect_from_items(nested, ranges);
                }
            }
            _ => {}
        }
    }
}

fn collect_struct(item: &syn::ItemStruct, ranges: &mut Vec<(usize, usize)>) {
    let owner = is_clap_owner(&item.attrs);
    if owner {
        push_docs(&item.attrs, ranges);
    }
    for field in &item.fields {
        if owner || is_clap_owner(&field.attrs) {
            push_docs(&field.attrs, ranges);
        }
    }
}

fn collect_enum(item: &syn::ItemEnum, ranges: &mut Vec<(usize, usize)>) {
    let owner = is_clap_owner(&item.attrs);
    if owner {
        push_docs(&item.attrs, ranges);
    }
    for variant in &item.variants {
        let variant_owner = owner || is_clap_owner(&variant.attrs);
        if variant_owner {
            push_docs(&variant.attrs, ranges);
        }
        for field in &variant.fields {
            if variant_owner || is_clap_owner(&field.attrs) {
                push_docs(&field.attrs, ranges);
            }
        }
    }
}

fn push_docs(attrs: &[syn::Attribute], ranges: &mut Vec<(usize, usize)>) {
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let range = attr.span().byte_range();
        if range.end > range.start {
            ranges.push((range.start, range.end));
        }
    }
}

fn is_clap_owner(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| is_clap_derive(attr) || is_clap_attr(attr))
}

fn is_clap_derive(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("derive") {
        return false;
    }
    attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        .ok()
        .is_some_and(|paths| paths.iter().any(path_is_clap_derive))
}

fn path_is_clap_derive(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|seg| {
        seg.ident == "Parser"
            || seg.ident == "Subcommand"
            || seg.ident == "Args"
            || seg.ident == "ValueEnum"
    })
}

fn is_clap_attr(attr: &syn::Attribute) -> bool {
    attr.path().segments.last().is_some_and(|seg| {
        seg.ident == "command"
            || seg.ident == "arg"
            || seg.ident == "clap"
            || seg.ident == "group"
            || seg.ident == "value"
    })
}
