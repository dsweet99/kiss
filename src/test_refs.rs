use crate::graph::DependencyGraph;
#[cfg(test)]
use crate::graph::build_dependency_graph;
use crate::parsing::ParsedFile;
use crate::units::{CodeUnitKind, get_child_by_field};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

#[derive(Debug, Clone)]
pub struct CodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    pub containing_class: Option<String>,
}

/// (`test_file_path`, `test_function_name`) — e.g. (`"tests/test_utils.py"`, `"test_parse_empty"`)
pub type CoveringTest = (PathBuf, String);

type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug)]
pub struct TestRefAnalysis {
    pub definitions: Vec<CodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<CodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

fn has_python_test_naming(path: &Path) -> bool {
    let is_py = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"));
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            (name.starts_with("test_") && is_py)
                || (name.len() > 8 && name[..name.len() - 3].ends_with("_test") && is_py)
                || name == "conftest.py"
        })
}

#[must_use]
pub fn is_test_file(path: &std::path::Path) -> bool {
    has_python_test_naming(path)
}

fn is_test_framework(name: &str) -> bool {
    name == "pytest"
        || name == "unittest"
        || name.starts_with("pytest.")
        || name.starts_with("unittest.")
}

fn is_test_framework_import_from(child: Node, source: &str) -> bool {
    child
        .child_by_field_name("module_name")
        .map(|m| &source[m.start_byte()..m.end_byte()])
        .is_some_and(is_test_framework)
}

fn contains_test_module_name(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let name = match child.kind() {
            "dotted_name" => Some(&source[child.start_byte()..child.end_byte()]),
            "aliased_import" => child
                .child_by_field_name("name")
                .map(|n| &source[n.start_byte()..n.end_byte()]),
            _ => None,
        };
        if name.is_some_and(|n| n == "pytest" || n == "unittest") {
            return true;
        }
    }
    false
}

pub fn has_test_framework_import(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" if contains_test_module_name(child, source) => return true,
            "import_from_statement" if is_test_framework_import_from(child, source) => return true,
            _ => {}
        }
    }
    false
}

fn has_test_function_or_class(node: Node, source: &str) -> bool {
    match node.kind() {
        "function_definition" | "async_function_definition" if is_test_function(node, source) => {
            true
        }
        "class_definition" if is_test_class(node, source) => true,
        _ => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|child| has_test_function_or_class(child, source))
        }
    }
}

pub fn is_in_test_directory(path: &Path) -> bool {
    use std::ffi::OsStr;
    path.components()
        .any(|c| c.as_os_str() == OsStr::new("tests") || c.as_os_str() == OsStr::new("test"))
}

fn is_python_test_file(parsed: &ParsedFile) -> bool {
    if is_test_file(&parsed.path) || is_in_test_directory(&parsed.path) {
        return true;
    }
    let root = parsed.tree.root_node();
    has_test_framework_import(root, &parsed.source)
        && has_test_function_or_class(root, &parsed.source)
}

pub fn build_name_file_map<'a>(
    items: impl Iterator<Item = (&'a str, &'a Path)>,
) -> HashMap<String, HashSet<PathBuf>> {
    let mut map: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    for (name, file) in items {
        map.entry(name.to_string())
            .or_default()
            .insert(file.to_path_buf());
    }
    map
}

fn path_identifiers(file: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(parent) = file.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(os) = component
                && let Some(s) = os.to_str()
            {
                ids.push(s.to_string());
            }
        }
    }
    if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
        ids.push(stem.to_string());
    }
    ids
}

pub(crate) fn disambiguate_files(
    files: &HashSet<PathBuf>,
    refs: &HashSet<String>,
) -> Option<PathBuf> {
    let file_ids: Vec<(&PathBuf, Vec<String>)> =
        files.iter().map(|f| (f, path_identifiers(f))).collect();

    let mut id_file_count: HashMap<&str, usize> = HashMap::new();
    for (_, ids) in &file_ids {
        for id in ids {
            *id_file_count.entry(id.as_str()).or_default() += 1;
        }
    }

    let mut winner: Option<&PathBuf> = None;
    for (file, ids) in &file_ids {
        let has_unique = ids
            .iter()
            .any(|id| refs.contains(id) && id_file_count.get(id.as_str()).copied() == Some(1));
        if has_unique {
            if winner.is_some() {
                return None;
            }
            winner = Some(file);
        }
    }
    winner.cloned()
}

#[allow(clippy::type_complexity)]
fn disambiguate_files_graph_fallback(
    name: &str,
    files: &HashSet<PathBuf>,
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    graph: &DependencyGraph,
) -> Option<PathBuf> {
    let test_files_using_name: Vec<PathBuf> = per_test_usage
        .iter()
        .filter(|(_, test_funcs)| {
            test_funcs
                .iter()
                .any(|(_, usage_refs)| usage_refs.contains(name))
        })
        .map(|(p, _)| p.clone())
        .collect();
    if test_files_using_name.is_empty() {
        return None;
    }
    let candidate_modules: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|f| graph.module_for_path(f).map(|m| (f.clone(), m)))
        .collect();
    if candidate_modules.is_empty() || candidate_modules.len() != files.len() {
        return None;
    }
    let mut winner: Option<PathBuf> = None;
    for (cand_path, cand_module) in &candidate_modules {
        let has_importer = test_files_using_name.iter().any(|test_path| {
            graph
                .module_for_path(test_path)
                .is_some_and(|test_mod| graph.imports(test_mod.as_str(), cand_module))
        });
        if has_importer {
            if winner.is_some() {
                return None;
            }
            winner = Some(cand_path.clone());
        }
    }
    winner
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_disambiguation_map(
    name_files: &HashMap<String, HashSet<PathBuf>>,
    refs: &HashSet<String>,
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    graph: Option<&DependencyGraph>,
) -> HashMap<String, PathBuf> {
    name_files
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .filter_map(|(name, files)| {
            disambiguate_files(files, refs).or_else(|| {
                graph.and_then(|g| disambiguate_files_graph_fallback(name, files, per_test_usage, g))
            })
            .map(|f| (name.clone(), f))
        })
        .collect()
}

fn file_to_module_suffix(file: &Path) -> String {
    let mut parts = Vec::new();
    if let Some(parent) = file.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(os) = component
                && let Some(s) = os.to_str()
            {
                parts.push(s);
            }
        }
    }
    if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
        parts.push(stem);
    }
    parts.join(".")
}

fn module_suffix_matches(def_suffix: &str, import_module: &str) -> bool {
    def_suffix == import_module || def_suffix.ends_with(&format!(".{import_module}"))
}

fn is_covered_by_import(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
) -> bool {
    if !usage_refs.contains(&def.name) {
        return false;
    }
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    import_bindings.iter().any(|(import_module, names)| {
        names.contains(&def.name) && module_suffix_matches(def_suffix, import_module)
    })
}

fn is_definition_covered(
    def: &CodeDefinition,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
) -> bool {
    if is_covered_by_import(def, import_bindings, module_suffixes, usage_refs) {
        return true;
    }
    if usage_refs.contains(&def.name) {
        let unique = name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
        if unique {
            return true;
        }
        if let Some(winner) = disambiguation.get(&def.name)
            && *winner == def.file
        {
            return true;
        }
    }
    if let Some(ref cls) = def.containing_class {
        return usage_refs.contains(cls);
    }
    false
}

#[allow(clippy::too_many_lines)]
pub fn analyze_test_refs(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, graph, true)
}

pub fn analyze_test_refs_quick(
    parsed_files: &[&ParsedFile],
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, None, false)
}

pub fn analyze_test_refs_no_map(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> TestRefAnalysis {
    analyze_test_refs_inner(parsed_files, graph, false)
}

type CollectedRefs = (
    Vec<CodeDefinition>,
    HashSet<String>,
    HashSet<String>,
    HashMap<String, HashSet<String>>,
    PerTestUsage,
);

fn collect_refs_parallel(parsed_files: &[&ParsedFile], need_coverage_map: bool) -> CollectedRefs {
    struct PerFileResult {
        definitions: Vec<CodeDefinition>,
        test_references: HashSet<String>,
        usage_references: HashSet<String>,
        import_bindings: HashMap<String, HashSet<String>>,
        per_test_usage: PerTestUsage,
    }

    parsed_files
        .par_iter()
        .map(|parsed| {
            let mut r = PerFileResult {
                definitions: Vec::new(),
                test_references: HashSet::new(),
                usage_references: HashSet::new(),
                import_bindings: HashMap::new(),
                per_test_usage: Vec::new(),
            };
            if is_python_test_file(parsed) {
                collect_all_test_file_data(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &mut r.test_references,
                    &mut r.usage_references,
                    &mut r.import_bindings,
                );
                if need_coverage_map {
                    let mut test_funcs = Vec::new();
                    collect_test_functions_with_refs(
                        parsed.tree.root_node(),
                        &parsed.source,
                        "",
                        &mut test_funcs,
                    );
                    r.per_test_usage = vec![(parsed.path.clone(), test_funcs)];
                }
            } else {
                collect_definitions(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &parsed.path,
                    &mut r.definitions,
                    false,
                    None,
                );
            }
            r
        })
        .fold(
            || {
                (
                    Vec::<CodeDefinition>::new(),
                    HashSet::<String>::new(),
                    HashSet::<String>::new(),
                    HashMap::<String, HashSet<String>>::new(),
                    PerTestUsage::new(),
                )
            },
            |(mut defs, mut t_refs, mut u_refs, mut i_binds, mut pt), r| {
                defs.extend(r.definitions);
                t_refs.extend(r.test_references);
                u_refs.extend(r.usage_references);
                for (module, names) in r.import_bindings {
                    i_binds.entry(module).or_default().extend(names);
                }
                pt.extend(r.per_test_usage);
                (defs, t_refs, u_refs, i_binds, pt)
            },
        )
        .reduce(
            || {
                (
                    Vec::<CodeDefinition>::new(),
                    HashSet::<String>::new(),
                    HashSet::<String>::new(),
                    HashMap::<String, HashSet<String>>::new(),
                    PerTestUsage::new(),
                )
            },
            |(mut defs, mut t_refs, mut u_refs, mut i_binds, mut pt),
             (defs2, t_refs2, u_refs2, i_binds2, pt2)| {
                defs.extend(defs2);
                t_refs.extend(t_refs2);
                u_refs.extend(u_refs2);
                for (module, names) in i_binds2 {
                    i_binds.entry(module).or_default().extend(names);
                }
                pt.extend(pt2);
                (defs, t_refs, u_refs, i_binds, pt)
            },
        )
}

fn analyze_test_refs_inner(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
    need_coverage_map: bool,
) -> TestRefAnalysis {
    let (definitions, test_references, usage_references, import_bindings, per_test_usage) =
        collect_refs_parallel(parsed_files, need_coverage_map);

    let name_files = build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation = build_disambiguation_map(
        &name_files,
        &test_references,
        &per_test_usage,
        graph,
    );
    let module_suffixes: HashMap<PathBuf, String> = definitions
        .iter()
        .map(|d| (d.file.clone(), file_to_module_suffix(&d.file)))
        .collect();

    let unreferenced = definitions
        .iter()
        .filter(|def| {
            !is_definition_covered(
                def,
                &name_files,
                &disambiguation,
                &import_bindings,
                &module_suffixes,
                &usage_references,
            )
        })
        .cloned()
        .collect();

    let coverage_map = if need_coverage_map {
        build_py_coverage_map(
            &definitions,
            &per_test_usage,
            &name_files,
            &disambiguation,
            &import_bindings,
            &module_suffixes,
        )
    } else {
        HashMap::new()
    };

    TestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
        coverage_map,
    }
}

#[allow(clippy::type_complexity)]
fn build_py_coverage_map(
    definitions: &[CodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            for def in definitions {
                if is_definition_covered(
                    def,
                    name_files,
                    disambiguation,
                    import_bindings,
                    module_suffixes,
                    usage_refs,
                ) {
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
}

fn try_add_def(
    node: Node,
    source: &str,
    file: &Path,
    defs: &mut Vec<CodeDefinition>,
    kind: CodeUnitKind,
    containing_class: Option<String>,
) {
    if let Some(name) = get_child_by_field(node, "name", source)
        && (!name.starts_with('_') || name == "__init__")
        && !name.starts_with("test_")
    {
        defs.push(CodeDefinition {
            name,
            kind,
            file: file.to_path_buf(),
            line: node.start_position().row + 1,
            containing_class,
        });
    }
}

fn is_protocol_class(node: Node, source: &str) -> bool {
    let Some(superclasses) = node.child_by_field_name("superclasses") else {
        return false;
    };
    let mut cursor = superclasses.walk();
    for child in superclasses.children(&mut cursor) {
        let text = &source[child.start_byte()..child.end_byte()];
        if text == "Protocol" || text == "typing.Protocol" {
            return true;
        }
    }
    false
}

fn is_abstract_method(node: Node, source: &str) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "decorated_definition" {
        return false;
    }
    let mut cursor = parent.walk();
    parent.children(&mut cursor).any(|child| {
        child.kind() == "decorator"
            && source[child.start_byte()..child.end_byte()].ends_with("abstractmethod")
    })
}

fn collect_definitions(
    node: Node,
    source: &str,
    file: &Path,
    defs: &mut Vec<CodeDefinition>,
    inside_class: bool,
    class_name: Option<&str>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" if is_abstract_method(node, source) => {
        }
        "function_definition" | "async_function_definition" => {
            let kind = if inside_class {
                CodeUnitKind::Method
            } else {
                CodeUnitKind::Function
            };
            try_add_def(node, source, file, defs, kind, class_name.map(String::from));
        }
        "class_definition" if is_protocol_class(node, source) => {}
        "class_definition" => {
            try_add_def(node, source, file, defs, CodeUnitKind::Class, None);
            let name = get_child_by_field(node, "name", source);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_definitions(child, source, file, defs, true, name.as_deref());
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_definitions(child, source, file, defs, inside_class, class_name);
            }
        }
    }
}

fn insert_identifier(node: Node, source: &str, refs: &mut HashSet<String>) {
    refs.insert(source[node.start_byte()..node.end_byte()].to_string());
}

fn is_test_function(node: Node, source: &str) -> bool {
    get_child_by_field(node, "name", source).is_some_and(|n| n.starts_with("test_"))
}

fn is_test_class(node: Node, source: &str) -> bool {
    get_child_by_field(node, "name", source).is_some_and(|n| n.starts_with("Test"))
}

/// Collects usage references (identifiers, call targets, type refs) from within a node.
/// Used to collect refs scoped to a single test function body.
fn collect_usage_refs_in_scope(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, refs);
            }
        }
        "type" => {
            collect_type_refs(node, source, refs);
        }
        "decorator" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier"
                    || child.kind() == "attribute"
                    || child.kind() == "call"
                {
                    collect_call_target(child, source, refs);
                }
            }
        }
        "identifier" => {
            insert_identifier(node, source, refs);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_usage_refs_in_scope(child, source, refs);
    }
}

/// Collects per-test (`test_id`, `usage_refs`) from a test file.
/// `test_id` format: `test_func` for module-level, `TestClass::test_method` for class methods.
fn collect_test_functions_with_refs(
    node: Node,
    source: &str,
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            if name.starts_with("test_") {
                let mut refs = HashSet::new();
                if let Some(body) = node.child_by_field_name("body") {
                    collect_usage_refs_in_scope(body, source, &mut refs);
                }
                let test_id = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}::{name}")
                };
                out.push((test_id, refs));
            }
        }
        "class_definition" => {
            let class_name = get_child_by_field(node, "name", source).unwrap_or_default();
            if class_name.starts_with("Test") {
                let class_prefix = if prefix.is_empty() {
                    class_name
                } else {
                    format!("{prefix}::{class_name}")
                };
                if let Some(body) = node.child_by_field_name("body") {
                    let mut cursor = body.walk();
                    for child in body.children(&mut cursor) {
                        if child.kind() == "function_definition"
                            || child.kind() == "async_function_definition"
                        {
                            let meth_name =
                                get_child_by_field(child, "name", source).unwrap_or_default();
                            if meth_name.starts_with("test_") {
                                let mut refs = HashSet::new();
                                if let Some(meth_body) = child.child_by_field_name("body") {
                                    collect_usage_refs_in_scope(meth_body, source, &mut refs);
                                }
                                let test_id = format!("{class_prefix}::{meth_name}");
                                out.push((test_id, refs));
                            }
                        }
                    }
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_test_functions_with_refs(child, source, prefix, out);
            }
        }
    }
}

fn collect_all_test_file_data(
    node: Node,
    source: &str,
    test_refs: &mut HashSet<String>,
    usage_refs: &mut HashSet<String>,
    import_bindings: &mut HashMap<String, HashSet<String>>,
) {
    match node.kind() {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, test_refs);
                collect_call_target(func, source, usage_refs);
            }
        }
        "import_from_statement" => {
            collect_import_names(node, source, test_refs);
            extract_import_from_binding(node, source, import_bindings);
            return;
        }
        "import_statement" => {
            collect_import_names(node, source, test_refs);
            return;
        }
        "type" => {
            collect_type_refs(node, source, test_refs);
            collect_type_refs(node, source, usage_refs);
            return;
        }
        "decorator" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier"
                    || child.kind() == "attribute"
                    || child.kind() == "call"
                {
                    collect_call_target(child, source, test_refs);
                    collect_call_target(child, source, usage_refs);
                }
            }
        }
        "identifier" => {
            insert_identifier(node, source, usage_refs);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_test_file_data(child, source, test_refs, usage_refs, import_bindings);
    }
}

fn collect_type_refs(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => insert_identifier(node, source, refs),
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                insert_identifier(attr, source, refs);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_refs(child, source, refs);
            }
        }
    }
}

fn collect_call_target(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => insert_identifier(node, source, refs),
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                insert_identifier(attr, source, refs);
            }
            if let Some(obj) = node.child_by_field_name("object") {
                collect_call_target(obj, source, refs);
            }
        }
        _ => {}
    }
}

fn extract_import_from_binding(
    node: Node,
    source: &str,
    bindings: &mut HashMap<String, HashSet<String>>,
) {
    let Some(module_node) = node.child_by_field_name("module_name") else {
        return;
    };
    let module_path = &source[module_node.start_byte()..module_node.end_byte()];
    if module_path.starts_with('.') {
        return;
    }

    let names = bindings.entry(module_path.to_string()).or_default();
    let module_id = module_node.id();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.id() == module_id {
            continue;
        }
        match child.kind() {
            "identifier" => {
                names.insert(source[child.start_byte()..child.end_byte()].to_string());
            }
            "aliased_import" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    names.insert(source[name_node.start_byte()..name_node.end_byte()].to_string());
                }
            }
            "dotted_name" => {
                let text = &source[child.start_byte()..child.end_byte()];
                if let Some(last) = text.rsplit('.').next() {
                    names.insert(last.to_string());
                }
            }
            _ => {}
        }
    }
}

fn collect_import_names(node: Node, source: &str, refs: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" | "aliased_import" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        insert_identifier(inner, source, refs);
                    }
                }
            }
            "identifier" => insert_identifier(child, source, refs),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_test_file_by_name() {
        assert!(is_test_file(Path::new("test_foo.py")));
        assert!(is_test_file(Path::new("foo_test.py")));
        assert!(is_test_file(Path::new("/some/path/test_bar.py")));
        assert!(!is_test_file(Path::new("foo.py")));
        assert!(!is_test_file(Path::new("testing.py")));
        assert!(!is_test_file(Path::new("my_test_helper.py")));
        assert!(
            !is_test_file(Path::new("test_foo.txt")),
            "non-.py should not match"
        );
        assert!(
            !is_test_file(Path::new("test_data.json")),
            "non-.py should not match"
        );
    }

    #[test]
    fn test_is_test_file_requires_naming_pattern() {
        assert!(is_test_file(Path::new("test_utils.py")));
        assert!(is_test_file(Path::new("utils_test.py")));
        assert!(is_test_file(Path::new("/project/tests/unit/test_utils.py")));
        assert!(
            is_test_file(Path::new("tests/conftest.py")),
            "conftest.py is pytest infrastructure"
        );
        assert!(
            is_test_file(Path::new("conftest.py")),
            "conftest.py at any level"
        );
        assert!(!is_test_file(Path::new("tests/helpers.py")));
        assert!(!is_test_file(Path::new("src/utils.py")));
        assert!(!is_test_file(Path::new("myproject/testing_utils.py")));
    }

    #[test]
    fn test_has_test_framework_import() {
        use crate::parsing::create_parser;
        let mut parser = create_parser().unwrap();

        let mut check = |src: &str| {
            let tree = parser.parse(src, None).unwrap();
            has_test_framework_import(tree.root_node(), src)
        };

        assert!(check("import pytest\n\ndef test_foo():\n    pass\n"));
        assert!(check(
            "import unittest\n\nclass TestCase(unittest.TestCase):\n    pass\n"
        ));
        assert!(check(
            "from pytest import fixture\n\n@fixture\ndef my_fixture():\n    pass\n"
        ));
        assert!(check("import pytest as pt\n"));
        assert!(!check("import os\nimport sys\n\ndef main():\n    pass\n"));
    }

    #[test]
    fn test_is_in_test_directory() {
        assert!(is_in_test_directory(Path::new("tests/helpers.py")));
        assert!(is_in_test_directory(Path::new("tests/unit/helpers.py")));
        assert!(is_in_test_directory(Path::new("test/helpers.py")));
        assert!(is_in_test_directory(Path::new(
            "/project/tests/conftest.py"
        )));
        assert!(!is_in_test_directory(Path::new("src/utils.py")));
        assert!(!is_in_test_directory(Path::new("testing/utils.py")));
    }

    #[test]
    fn test_collect_definitions_skips_test_functions() {
        use crate::parsing::create_parser;
        let mut parser = create_parser().unwrap();
        let src = "def helper():\n    pass\n\ndef test_helper():\n    pass\n";
        let tree = parser.parse(src, None).unwrap();
        let mut defs = Vec::new();
        collect_definitions(
            tree.root_node(),
            src,
            Path::new("utils.py"),
            &mut defs,
            false,
            None,
        );
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["helper"]);
    }

    #[test]
    fn test_nested_functions_not_tracked_for_coverage() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def outer():\n    def nested_helper():\n        return 42\n    return nested_helper()\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("mymodule.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "from mymodule import outer\ndef test_outer():\n    outer()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_mymodule.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);

        let def_names: Vec<&str> = analysis
            .definitions
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(
            !def_names.contains(&"nested_helper"),
            "Nested function should not be tracked for coverage, but found: {def_names:?}"
        );
    }

    #[test]
    fn test_file_stem_collision_no_false_positive() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src_utils = "def parse():\n    pass\n";
        let tree_utils = parser.parse(src_utils, None).unwrap();
        let file_utils = ParsedFile {
            path: PathBuf::from("utils.py"),
            source: src_utils.to_string(),
            tree: tree_utils,
        };

        let src_helpers = "def parse():\n    pass\n";
        let tree_helpers = parser.parse(src_helpers, None).unwrap();
        let file_helpers = ParsedFile {
            path: PathBuf::from("helpers.py"),
            source: src_helpers.to_string(),
            tree: tree_helpers,
        };

        let src_test = "from utils import parse\nimport helpers\ndef test_it():\n    parse()\n    helpers.do_stuff()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_stuff.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file_utils, &file_helpers, &file_test], None);

        let unref_files: Vec<&str> = analysis
            .unreferenced
            .iter()
            .map(|d| d.file.to_str().unwrap())
            .collect();
        assert!(
            unref_files.contains(&"helpers.py"),
            "helpers.parse should be uncovered (test doesn't exercise it): unreferenced={unref_files:?}"
        );
    }

    #[test]
    fn test_same_stem_as_function_name_different_dirs() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def some_name():\n    pass\n";

        let tree_1 = parser.parse(src, None).unwrap();
        let file_1 = ParsedFile {
            path: PathBuf::from("sub_dir_1/some_name.py"),
            source: src.to_string(),
            tree: tree_1,
        };

        let tree_2 = parser.parse(src, None).unwrap();
        let file_2 = ParsedFile {
            path: PathBuf::from("sub_dir_2/some_name.py"),
            source: src.to_string(),
            tree: tree_2,
        };

        let src_test =
            "from sub_dir_1.some_name import some_name\ndef test_it():\n    some_name()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_stuff.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file_1, &file_2, &file_test], None);

        let unref: Vec<_> = analysis
            .unreferenced
            .iter()
            .map(|d| d.file.to_str().unwrap())
            .collect();
        assert!(
            !unref.contains(&"sub_dir_1/some_name.py"),
            "sub_dir_1/some_name::some_name should be covered (explicitly imported and called): unreferenced={unref:?}"
        );
        assert!(
            unref.contains(&"sub_dir_2/some_name.py"),
            "sub_dir_2/some_name::some_name should be uncovered: unreferenced={unref:?}"
        );
    }

    #[test]
    fn test_import_module_without_from_falls_back() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def func():\n    pass\n";

        let tree_1 = parser.parse(src, None).unwrap();
        let file_1 = ParsedFile {
            path: PathBuf::from("alpha.py"),
            source: src.to_string(),
            tree: tree_1,
        };

        let tree_2 = parser.parse(src, None).unwrap();
        let file_2 = ParsedFile {
            path: PathBuf::from("beta.py"),
            source: src.to_string(),
            tree: tree_2,
        };

        let src_test = "import alpha\ndef test_it():\n    alpha.func()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_stuff.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file_1, &file_2, &file_test], None);

        let unref: Vec<_> = analysis
            .unreferenced
            .iter()
            .map(|d| d.file.to_str().unwrap())
            .collect();
        assert!(
            !unref.contains(&"alpha.py"),
            "`import alpha; alpha.func()` should cover alpha.func via fallback: unreferenced={unref:?}"
        );
        assert!(
            unref.contains(&"beta.py"),
            "beta.func should be uncovered: unreferenced={unref:?}"
        );
    }

    #[test]
    fn test_relative_import_falls_back_to_flat_refs() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def helper():\n    pass\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("mymod.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "from . import mymod\ndef test_it():\n    mymod.helper()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_mymod.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);

        assert!(
            analysis.unreferenced.is_empty(),
            "relative import should fall back to flat refs and cover helper: unreferenced={:?}",
            analysis
                .unreferenced
                .iter()
                .map(|d| &d.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_import_without_call_not_covered() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def some_func():\n    pass\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("mymod.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "from mymod import some_func\ndef test_it():\n    pass\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_mymod.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);
        assert!(
            !analysis.unreferenced.is_empty(),
            "some_func should be uncovered (imported but never called)"
        );
    }

    #[test]
    fn test_import_with_call_is_covered() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def some_func():\n    pass\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("mymod.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "from mymod import some_func\ndef test_it():\n    some_func()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_mymod.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);
        assert!(
            analysis.unreferenced.is_empty(),
            "some_func should be covered (imported AND called): unreferenced={:?}",
            analysis
                .unreferenced
                .iter()
                .map(|d| &d.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_class_import_without_use_not_covered() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "class MyClass:\n    def __init__(self):\n        pass\n    def process(self):\n        pass\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("mymod.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "from mymod import MyClass\ndef test_it():\n    pass\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_mymod.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);
        let unref_names: Vec<&str> = analysis
            .unreferenced
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(
            unref_names.contains(&"__init__"),
            "MyClass.__init__ should be uncovered (class imported but never used): unreferenced={unref_names:?}"
        );
        assert!(
            unref_names.contains(&"process"),
            "MyClass.process should be uncovered (class imported but never used): unreferenced={unref_names:?}"
        );
    }

    #[test]
    fn test_protocol_class_excluded_from_coverage() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "from typing import Protocol\n\nclass Readable(Protocol):\n    def read(self) -> str: ...\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("interfaces.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "def test_placeholder():\n    pass\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_interfaces.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);
        let def_names: Vec<&str> = analysis
            .definitions
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(
            !def_names.contains(&"Readable"),
            "Protocol class should not be tracked for coverage: definitions={def_names:?}"
        );
        assert!(
            !def_names.contains(&"read"),
            "Protocol method should not be tracked for coverage: definitions={def_names:?}"
        );
    }

    #[test]
    fn test_abstract_methods_excluded_from_coverage() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "from abc import ABC, abstractmethod\n\nclass MyBase(ABC):\n    @abstractmethod\n    def process(self):\n        pass\n\n    def concrete(self):\n        return 42\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("base.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "def test_placeholder():\n    pass\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_base.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);
        let def_names: Vec<&str> = analysis
            .definitions
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(
            !def_names.contains(&"process"),
            "Abstract method should not be tracked for coverage: definitions={def_names:?}"
        );
        assert!(
            def_names.contains(&"concrete"),
            "Concrete method should still be tracked: definitions={def_names:?}"
        );
    }

    #[test]
    fn test_coverage_map_one_function_covered_by_two_tests() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src = "def parse(x):\n    return int(x or 0)\n";
        let tree = parser.parse(src, None).unwrap();
        let file = ParsedFile {
            path: PathBuf::from("utils.py"),
            source: src.to_string(),
            tree,
        };

        let src_test = "from utils import parse\n\ndef test_parse_empty():\n    assert parse('') == 0\n\ndef test_parse_valid():\n    assert parse('42') == 42\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_utils.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file, &file_test], None);
        let key = (PathBuf::from("utils.py"), "parse".to_string());
        let covering = analysis.coverage_map.get(&key).expect("coverage_map should have parse");
        let test_ids: Vec<String> = covering
            .iter()
            .map(|(_, func)| func.clone())
            .collect();
        assert!(
            test_ids.contains(&"test_parse_empty".to_string()),
            "coverage_map should list test_parse_empty, got: {test_ids:?}"
        );
        assert!(
            test_ids.contains(&"test_parse_valid".to_string()),
            "coverage_map should list test_parse_valid, got: {test_ids:?}"
        );
        assert_eq!(covering.len(), 2);
    }

    #[test]
    fn test_same_name_different_files_disambiguated_by_module() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src_a = "def helper():\n    pass\n";
        let tree_a = parser.parse(src_a, None).unwrap();
        let file_a = ParsedFile {
            path: PathBuf::from("alpha.py"),
            source: src_a.to_string(),
            tree: tree_a,
        };

        let src_b = "def helper():\n    pass\n";
        let tree_b = parser.parse(src_b, None).unwrap();
        let file_b = ParsedFile {
            path: PathBuf::from("beta.py"),
            source: src_b.to_string(),
            tree: tree_b,
        };

        let src_test = "from alpha import helper\ndef test_it():\n    helper()\n";
        let tree_test = parser.parse(src_test, None).unwrap();
        let file_test = ParsedFile {
            path: PathBuf::from("test_alpha.py"),
            source: src_test.to_string(),
            tree: tree_test,
        };

        let analysis = analyze_test_refs(&[&file_a, &file_b, &file_test], None);

        assert_eq!(analysis.definitions.len(), 2, "both files define helper()");

        let unref_files: Vec<&str> = analysis
            .unreferenced
            .iter()
            .map(|d| d.file.to_str().unwrap())
            .collect();
        assert!(
            !unref_files.contains(&"alpha.py"),
            "alpha.helper should be covered (test imports from alpha)"
        );
        assert!(
            unref_files.contains(&"beta.py"),
            "beta.helper should be uncovered (no test references beta)"
        );
    }

    /// Exercises `disambiguate_files_graph_fallback`: when ref-based disambiguation ties
    /// (both alpha and beta appear in refs), the graph picks the module imported by the
    /// test that uses the name.
    #[test]
    fn test_disambiguate_by_graph_when_refs_tie() {
        use crate::parsing::{ParsedFile, create_parser};
        let mut parser = create_parser().unwrap();

        let src_a = "def helper():\n    pass\n";
        let tree_a = parser.parse(src_a, None).unwrap();
        let file_a = ParsedFile {
            path: PathBuf::from("alpha.py"),
            source: src_a.to_string(),
            tree: tree_a,
        };

        let src_b = "def helper():\n    pass\n";
        let tree_b = parser.parse(src_b, None).unwrap();
        let file_b = ParsedFile {
            path: PathBuf::from("beta.py"),
            source: src_b.to_string(),
            tree: tree_b,
        };

        let src_test_a = "from alpha import helper\ndef test_a():\n    helper()\n";
        let tree_test_a = parser.parse(src_test_a, None).unwrap();
        let file_test_a = ParsedFile {
            path: PathBuf::from("tests/test_a.py"),
            source: src_test_a.to_string(),
            tree: tree_test_a,
        };

        let src_test_b = "import beta\n";
        let tree_test_b = parser.parse(src_test_b, None).unwrap();
        let file_test_b = ParsedFile {
            path: PathBuf::from("tests/test_b.py"),
            source: src_test_b.to_string(),
            tree: tree_test_b,
        };

        let parsed: Vec<&ParsedFile> = vec![&file_a, &file_b, &file_test_a, &file_test_b];
        let graph = build_dependency_graph(&parsed);
        let analysis = analyze_test_refs(&parsed, Some(&graph));

        assert_eq!(analysis.definitions.len(), 2, "both files define helper()");

        let unref_files: Vec<&str> = analysis
            .unreferenced
            .iter()
            .map(|d| d.file.to_str().unwrap())
            .collect();
        assert!(
            !unref_files.contains(&"alpha.py"),
            "alpha.helper should be covered (graph fallback: test_a imports alpha)"
        );
        assert!(
            unref_files.contains(&"beta.py"),
            "beta.helper should be uncovered (no test imports and uses beta)"
        );
    }
}
