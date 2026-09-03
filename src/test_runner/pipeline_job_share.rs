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
        let (python_jobs, rust_jobs) = split_jobs(self.total, self.both);
        match language {
            Language::Python => python_jobs,
            Language::Rust => rust_jobs,
        }
    }

    pub(super) fn acquire_execute(&self, language: Language) -> ExecuteTurn {
        ExecuteTurn {
            jobs: self.covering(language),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JobShare;
    use kiss::Language;
    use std::sync::Barrier;

    #[test]
    fn both_languages_execute_concurrently_with_half_budget() {
        let share = JobShare::new(4, true);
        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            let rust = scope.spawn(|| {
                let turn = share.acquire_execute(Language::Rust);
                assert_eq!(turn.jobs, 2);
                barrier.wait();
            });
            let python = share.acquire_execute(Language::Python);
            assert_eq!(python.jobs, 2);
            barrier.wait();
            rust.join().unwrap();
        });
    }

    #[test]
    fn one_language_executes_with_full_budget() {
        let share = JobShare::new(4, false);
        assert_eq!(share.acquire_execute(Language::Python).jobs, 4);
        assert_eq!(share.acquire_execute(Language::Rust).jobs, 4);
    }
}
