#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlobalMetrics {
    pub files: usize,
    pub code_units: usize,
    pub statements: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
}

#[cfg(test)]
mod tests {
    use super::GlobalMetrics;

    #[test]
    fn default_metrics_are_zero() {
        assert_eq!(
            GlobalMetrics::default(),
            GlobalMetrics {
                files: 0,
                code_units: 0,
                statements: 0,
                graph_nodes: 0,
                graph_edges: 0,
            }
        );
    }
}
