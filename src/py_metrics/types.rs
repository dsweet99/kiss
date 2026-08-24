#[derive(Debug, Default)]
pub struct FunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub arguments_positional: usize,
    pub arguments_keyword_only: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
    pub max_try_block_statements: usize,
    pub boolean_parameters: usize,
    pub decorators: usize,
    pub max_return_values: usize,
    pub calls: usize,
    pub has_error: bool,
}

#[derive(Debug, Default)]
pub struct ClassMetrics {
    pub methods: usize,
}

#[derive(Debug, Default)]
pub struct FileMetrics {
    pub statements: usize,
    pub interface_types: usize,
    pub concrete_types: usize,
    pub imports: usize,
    pub functions: usize,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl FileMetrics {
        fn witness() -> Self {
            Self::default()
        }
    }

    #[test]
    fn witness_file_metrics() {
        let _ = FileMetrics::witness();
    }
}
