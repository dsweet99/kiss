use kiss::Language;

pub struct DiscoverArgs<'a> {
    pub universe: &'a str,
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl DiscoverArgs<'_> {
        fn witness() -> Self {
            Self {
                universe: ".",
                paths: &[],
                lang_filter: None,
                ignore: &[],
            }
        }
    }

    #[test]
    fn witness_discover_args() {
        let _ = DiscoverArgs::witness();
    }
}
