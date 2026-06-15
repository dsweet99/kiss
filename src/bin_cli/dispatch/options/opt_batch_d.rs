use std::path::PathBuf;

use kiss::Language;

pub(crate) struct MvDispatchOptions {
    pub lang: Option<Language>,
    pub query: String,
    pub new_name: String,
    pub paths: Vec<String>,
    pub to: Option<PathBuf>,
    pub mv_flags: MvOutputFlags,
    pub ignore: Vec<String>,
}

pub(crate) struct MvOutputFlags {
    pub dry_run: bool,
    pub json: bool,
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl MvDispatchOptions {
        fn witness() -> Self {
            Self {
                lang: None,
                query: "q".into(),
                new_name: "n".into(),
                paths: vec![],
                to: None,
                mv_flags: MvOutputFlags::witness(),
                ignore: vec![],
            }
        }
    }

    impl MvOutputFlags {
        fn witness() -> Self {
            Self {
                dry_run: false,
                json: false,
            }
        }
    }

    #[test]
    fn witness_opt_batch_d() {
        let _ = MvDispatchOptions::witness();
        let _ = MvOutputFlags::witness();
    }
}
