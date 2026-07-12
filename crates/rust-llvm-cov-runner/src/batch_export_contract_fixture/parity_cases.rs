#[cfg(test)]
pub(crate) struct ParityMatrixCase {
    pub name: &'static str,
    pub selectors: &'static [&'static str],
    pub test_args: &'static [&'static str],
    pub jobs: usize,
}

#[cfg(test)]
pub(crate) fn parity_matrix_cases() -> &'static [ParityMatrixCase] {
    &[
        ParityMatrixCase {
            name: "substring-and-cross-package",
            selectors: &[
                "invokes_helper_in_process",
                "spawns_instrumented_helper_binary",
                "helper",
            ],
            test_args: &[],
            jobs: 2,
        },
        ParityMatrixCase {
            name: "exact-match",
            selectors: &["invokes_helper_in_process"],
            test_args: &["--exact"],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "substring-collision",
            selectors: &["alpha", "alphabet"],
            test_args: &[],
            jobs: 2,
        },
        ParityMatrixCase {
            name: "multi-selector-one-instance",
            selectors: &["helper", "invokes_helper"],
            test_args: &[],
            jobs: 2,
        },
        ParityMatrixCase {
            name: "stdout-stderr",
            selectors: &["prints_stdout_and_stderr"],
            test_args: &[],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "failure",
            selectors: &["fails_assertion_for_parity"],
            test_args: &[],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "ignored",
            selectors: &["ignored_coverage_case"],
            test_args: &["--ignored"],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "skip",
            selectors: &["invokes_helper_in_process"],
            test_args: &["--skip", "fails_assertion_for_parity"],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "multi-binary",
            selectors: &["lib_helper_value", "invokes_helper_in_process"],
            test_args: &[],
            jobs: 2,
        },
        ParityMatrixCase {
            name: "diagnostic-exit-37",
            selectors: &["exits_with_diagnostic_code_37"],
            test_args: &["--exact"],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "concurrency-bound",
            selectors: &[
                "invokes_helper_in_process",
                "spawns_instrumented_helper_binary",
                "prints_stdout_and_stderr",
                "substring_collision_alpha",
                "substring_collision_alphabet",
                "lib_helper_value",
            ],
            test_args: &[],
            jobs: 2,
        },
        ParityMatrixCase {
            name: "unmatched-selector",
            selectors: &["selector_that_matches_nothing_in_fixture"],
            test_args: &[],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "mixed-matched-unmatched",
            selectors: &["invokes_helper_in_process", "selector_that_matches_nothing_in_fixture"],
            test_args: &[],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "exact-prefix-zero-instances",
            selectors: &["invokes"],
            test_args: &["--exact"],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "nocapture-live-output",
            selectors: &["prints_stdout_and_stderr"],
            test_args: &["--nocapture"],
            jobs: 1,
        },
        ParityMatrixCase {
            name: "suite-failure-status-agreement",
            selectors: &["fails_assertion_for_parity", "invokes_helper_in_process"],
            test_args: &[],
            jobs: 1,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{ParityMatrixCase, parity_matrix_cases};

    #[test]
    fn parity_matrix_case_catalog_is_non_empty() {
        assert!(!parity_matrix_cases().is_empty());
        assert_eq!(parity_matrix_cases()[0].name, "substring-and-cross-package");
    }

    #[test]
    fn parity_matrix_case_fields_are_populated() {
        let case = ParityMatrixCase {
            name: "witness",
            selectors: &["alpha"],
            test_args: &["--exact"],
            jobs: 3,
        };
        assert_eq!(case.name, "witness");
        assert_eq!(case.selectors, ["alpha"]);
        assert_eq!(case.test_args, ["--exact"]);
        assert_eq!(case.jobs, 3);
        let catalog = &parity_matrix_cases()[1];
        assert_eq!(catalog.name, "exact-match");
        assert_eq!(catalog.test_args, ["--exact"]);
    }
}
