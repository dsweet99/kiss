pub(crate) struct ImportInfo {
    pub from_qualified: String,
    pub from_parent_module: Option<String>,
    pub imports: Vec<String>,
}

pub(crate) struct GraphBuildState<'a> {
    pub graph: &'a mut DependencyGraph,
    pub bare_to_qualified: &'a mut HashMap<String, Vec<String>>,
}

impl GraphBuildState<'_> {
    pub(crate) fn register_module(&mut self, path: &Path, qualified: String, bare: String) {
        self.graph
            .path_to_module
            .insert(path.to_path_buf(), qualified.clone());
        self.graph.paths.insert(qualified.clone(), path.to_path_buf());
        self.graph.get_or_create_node(&qualified);
        self.bare_to_qualified
            .entry(bare)
            .or_default()
            .push(qualified);
    }
}

pub(crate) fn add_edges_for_import_info(
    graph: &mut DependencyGraph,
    info: &ImportInfo,
    bare_to_qualified: &HashMap<String, Vec<String>>,
) {
    for import in &info.imports {
        if graph.nodes.contains_key(import) {
            graph.add_dependency(&info.from_qualified, import);
            continue;
        }
        let resolved = resolve_import(
            import,
            info.from_parent_module.as_deref(),
            bare_to_qualified,
        );
        for r in resolved {
            graph.add_dependency(&info.from_qualified, &r);
        }
    }
}

#[must_use]
pub fn build_dependency_graph(parsed_files: &[&ParsedFile]) -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

    {
        let mut state = GraphBuildState {
            graph: &mut graph,
            bare_to_qualified: &mut bare_to_qualified,
        };
        for parsed in parsed_files {
            let qualified = qualified_module_name(&parsed.path);
            let bare = bare_module_name(&parsed.path);
            state.register_module(&parsed.path, qualified, bare);
        }
    }

    let per_file: Vec<ImportInfo> = parsed_files
        .par_iter()
        .map(|parsed| {
            let fq = qualified_module_name(&parsed.path);
            ImportInfo {
                from_qualified: fq.clone(),
                from_parent_module: fq.rsplit_once('.').map(|(p, _)| p.to_string()),
                imports: extract_imports_for_cache(parsed.tree.root_node(), &parsed.source),
            }
        })
        .collect();

    for info in &per_file {
        add_edges_for_import_info(&mut graph, info, &bare_to_qualified);
    }
    graph
}

pub(crate) struct ImportListPass<'a> {
    pub graph: &'a mut DependencyGraph,
    pub bare_to_qualified: &'a HashMap<String, Vec<String>>,
}

impl ImportListPass<'_> {
    pub(crate) fn add_edges(
        &mut self,
        from_qualified: &str,
        from_parent: Option<&str>,
        imports: &[String],
    ) {
        for import in imports {
            if self.graph.nodes.contains_key(import) {
                self.graph.add_dependency(from_qualified, import);
                continue;
            }
            let resolved = resolve_import(import, from_parent, self.bare_to_qualified);
            for r in resolved {
                self.graph.add_dependency(from_qualified, &r);
            }
        }
    }
}

#[must_use]
pub fn build_dependency_graph_from_import_lists(
    files: &[(PathBuf, Vec<String>)],
) -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();

    {
        let mut state = GraphBuildState {
            graph: &mut graph,
            bare_to_qualified: &mut bare_to_qualified,
        };
        for (path, _) in files {
            let qualified = qualified_module_name(path);
            let bare = bare_module_name(path);
            state.register_module(path, qualified, bare);
        }
    }

    let mut pass = ImportListPass {
        graph: &mut graph,
        bare_to_qualified: &bare_to_qualified,
    };
    for (path, imports) in files {
        let from_qualified = qualified_module_name(path);
        let from_parent = from_qualified.rsplit_once('.').map(|(p, _)| p);
        pass.add_edges(&from_qualified, from_parent, imports);
    }
    graph
}

pub(crate) fn parent_prefix_match(candidates: &[String], parent: Option<&str>) -> Option<String> {
    let prefix = format!("{}.", parent?);
    let mut hits = candidates.iter().filter(|c| c.starts_with(&prefix));
    let first = hits.next()?.clone();
    if hits.next().is_some() {
        return None;
    }
    Some(first)
}

pub(crate) fn resolve_bare(candidates: &[String], parent: Option<&str>) -> Vec<String> {
    let mut deduped = candidates.to_vec();
    deduped.sort();
    deduped.dedup();
    if deduped.len() == 1 {
        return deduped;
    }
    parent_prefix_match(&deduped, parent).into_iter().collect()
}

pub(crate) fn resolve_dotted(
    import: &str,
    parent: Option<&str>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let Some((_, last)) = import.rsplit_once('.') else {
        return Vec::new();
    };
    let Some(candidates) = bare_to_qualified.get(last) else {
        return Vec::new();
    };
    let mut matches: Vec<String> = candidates
        .iter()
        .filter(|c| c.ends_with(import))
        .cloned()
        .collect();
    if matches.len() == 1 {
        return matches;
    }
    let pool = if matches.is_empty() {
        candidates.as_slice()
    } else {
        matches.as_slice()
    };
    if let Some(hit) = parent_prefix_match(pool, parent) {
        return vec![hit];
    }
    matches.sort();
    matches.dedup();
    matches
}

pub(crate) fn resolve_import(
    import: &str,
    from_parent_module: Option<&str>,
    bare_to_qualified: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(candidates) = bare_to_qualified.get(import) {
        return resolve_bare(candidates, from_parent_module);
    }
    resolve_dotted(import, from_parent_module, bare_to_qualified)
}
