use kiss::Language;

pub(super) struct JobShare {
    total: usize,
}

pub(super) struct ExecuteTurn {
    pub jobs: usize,
}

impl JobShare {
    pub(super) fn new(total: usize) -> Self {
        Self {
            total: total.max(1),
        }
    }

    pub(super) fn acquire_execute(&self, language: Language) -> ExecuteTurn {
        let _ = language;
        ExecuteTurn { jobs: self.total }
    }
}

#[cfg(test)]
#[path = "pipeline_job_share_test.rs"]
mod tests;
