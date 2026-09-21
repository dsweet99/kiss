use std::path::{Path, PathBuf};

#[cfg(unix)]
use super::control::NudgeReplyMsg;
#[cfg(not(unix))]
use super::session_cycle::NudgeReplyMsg;
use super::reload::WatchLiveConfig;

#[derive(Clone, Default)]
pub(super) struct LastReplies {
    pub(super) repo: PathBuf,
    pub(super) ignore: Vec<String>,
    pub(super) extra: Vec<String>,
    pub(super) python_extra: Vec<String>,
    all: Option<NudgeReplyMsg>,
    python: Option<NudgeReplyMsg>,
    rust: Option<NudgeReplyMsg>,
}

impl LastReplies {
    pub(super) fn for_repo(repo: &Path) -> Self {
        Self {
            repo: repo.to_path_buf(),
            ..Self::default()
        }
    }

    pub(super) fn for_session(repo: &Path, live: &WatchLiveConfig) -> Self {
        let mut last = Self::for_repo(repo);
        last.stamp_session(&live.ignore, &live.extra, &live.python_extra);
        last
    }

    pub(super) fn stamp_session(
        &mut self, ignore: &[String], extra: &[String], python_extra: &[String],
    ) {
        let changed =
            self.ignore != ignore || self.extra != extra || self.python_extra != python_extra;
        self.ignore = ignore.to_vec();
        self.extra = extra.to_vec();
        self.python_extra = python_extra.to_vec();
        if changed {
            self.all = None;
            self.python = None;
            self.rust = None;
        }
    }

    pub(super) fn matches_args(&self, args: &crate::test_runner::RunTestCmdArgs<'_>) -> bool {
        self.ignore == args.ignore
            && self.extra == args.extra
            && self.python_extra == args.python_extra
    }

    pub(super) fn get(&self, lang: Option<kiss::Language>) -> Option<&NudgeReplyMsg> {
        match lang {
            None => self.all.as_ref(),
            Some(kiss::Language::Python) => self.python.as_ref(),
            Some(kiss::Language::Rust) => self.rust.as_ref(),
        }
    }

    pub(super) fn store(&mut self, lang: Option<kiss::Language>, msg: NudgeReplyMsg) {
        match lang {
            None => self.all = Some(msg),
            Some(kiss::Language::Python) => self.python = Some(msg),
            Some(kiss::Language::Rust) => self.rust = Some(msg),
        }
    }

    pub(super) fn clone_any(&self) -> Option<NudgeReplyMsg> {
        self.all
            .clone()
            .or_else(|| self.python.clone())
            .or_else(|| self.rust.clone())
    }

    pub(super) fn store_named_language_slices(
        &mut self,
        suite: &kiss::rust_llvm_cov_runner::WatchSuiteReport,
        bilingual: &NudgeReplyMsg,
    ) {
        for lang in [kiss::Language::Python, kiss::Language::Rust] {
            let sliced = suite.try_format_language(lang).or_else(|| {
                crate::test_runner::durable_lang_reply(
                    &self.repo, lang, &self.ignore, &self.extra, &self.python_extra,
                )
            });
            let Some((exit_code, output)) = sliced else {
                continue;
            };
            self.store(
                Some(lang),
                NudgeReplyMsg {
                    exit_code,
                    pid: bilingual.pid,
                    error: None,
                    output: Some(output),
                    ..NudgeReplyMsg::default()
                },
            );
        }
    }
}
