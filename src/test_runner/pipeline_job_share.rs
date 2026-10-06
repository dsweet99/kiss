use kiss::Language;

use crate::test_runner::pipeline::split_jobs;

pub(super) struct JobShare {
    total: usize,
    both: bool,
}

pub(super) struct ExecuteTurn {
    pub jobs: usize,
}

impl JobShare {
    pub(super) fn new(total: usize, both: bool) -> Self {
        Self {
            total: total.max(1),
            both,
        }
    }

    pub(super) fn covering(&self, language: Language) -> usize {
        let (python, rust) = split_jobs(self.total, self.both);
        *crate::test_runner::language_keyed::LanguageKeyed { python, rust }.get(language)
    }

    pub(super) fn acquire_execute(&self, language: Language) -> ExecuteTurn {
        let _ = language;
        ExecuteTurn { jobs: self.total }
    }
}

#[cfg(test)]
#[path = "pipeline_job_share_test.rs"]
mod tests;
