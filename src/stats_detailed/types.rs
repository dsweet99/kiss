/// Per-unit metrics carried into `kiss stats --table` and `kiss stats --all`.
///
/// Field names are internal; metric IDs surfaced to the user (e.g. on `STAT:` lines)
/// come from the canonical registry `kiss::stats::METRICS`. Each `Option<usize>` is
/// `Some` when the metric is meaningful for the unit's `kind` and the underlying
/// language collector populates it; otherwise `None` so the metric is skipped for
/// that unit when computing top-N outliers.
///
/// Construct via `UnitMetrics::new(file, name, kind, line)` and set the metrics
/// that apply to the unit; everything else stays `None`.
#[derive(Debug, Clone)]
pub struct UnitMetrics {
    pub file: String,
    pub name: String,
    pub kind: &'static str,
    pub line: usize,
    // function-scope
    pub statements: Option<usize>,
    pub arguments: Option<usize>,
    pub args_positional: Option<usize>,
    pub args_keyword_only: Option<usize>,
    pub indentation: Option<usize>,
    pub nested_depth: Option<usize>,
    pub branches: Option<usize>,
    pub returns: Option<usize>,
    pub return_values: Option<usize>,
    pub locals: Option<usize>,
    pub try_block_statements: Option<usize>,
    pub boolean_parameters: Option<usize>,
    pub annotations: Option<usize>,
    pub calls: Option<usize>,
    // type-scope
    pub methods: Option<usize>,
    // file-scope
    pub lines: Option<usize>,
    pub imports: Option<usize>,
    pub file_statements: Option<usize>,
    pub file_functions: Option<usize>,
    pub interface_types: Option<usize>,
    pub concrete_types: Option<usize>,
    // module-scope
    pub fan_in: Option<usize>,
    pub fan_out: Option<usize>,
    pub indirect_deps: Option<usize>,
    pub dependency_depth: Option<usize>,
}

impl UnitMetrics {
    #[must_use]
    pub const fn new(file: String, name: String, kind: &'static str, line: usize) -> Self {
        Self {
            file,
            name,
            kind,
            line,
            statements: None,
            arguments: None,
            args_positional: None,
            args_keyword_only: None,
            indentation: None,
            nested_depth: None,
            branches: None,
            returns: None,
            return_values: None,
            locals: None,
            try_block_statements: None,
            boolean_parameters: None,
            annotations: None,
            calls: None,
            methods: None,
            lines: None,
            imports: None,
            file_statements: None,
            file_functions: None,
            interface_types: None,
            concrete_types: None,
            fan_in: None,
            fan_out: None,
            indirect_deps: None,
            dependency_depth: None,
        }
    }
}
