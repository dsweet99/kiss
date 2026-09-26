use std::path::{Path, PathBuf};

#[cfg(unix)]
use super::control::NudgeReplyMsg;
use super::reload::WatchLiveConfig;
#[cfg(not(unix))]
use super::session_cycle::NudgeReplyMsg;
use crate::test_runner::language_keyed::LanguageKeyed;

#[derive(Clone, Default)]
pub(super) struct LastReplies {
    pub(super) repo: PathBuf,
    pub(super) ignore: Vec<String>,
    pub(super) extras: LanguageKeyed<Vec<String>>,
    all: Option<NudgeReplyMsg>,
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
        last.stamp_session(&live.target_request.ignore, &live.extras);
        last
    }

    pub(super) fn stamp_session(&mut self, ignore: &[String], extras: &LanguageKeyed<Vec<String>>) {
        let changed = self.ignore != ignore || self.extras != *extras;
        self.ignore = ignore.to_vec();
        self.extras = extras.clone();
        if changed {
            self.all = None;
        }
    }

    pub(super) fn matches_args(&self, args: &crate::test_runner::RunTestCmdArgs<'_>) -> bool {
        self.ignore == args.ignore() && self.extras.as_slices() == args.extras
    }

    #[cfg(test)]
    pub(super) fn get(&self, _lang: Option<kiss::Language>) -> Option<&NudgeReplyMsg> {
        self.all.as_ref()
    }

    pub(super) fn store(&mut self, _lang: Option<kiss::Language>, msg: NudgeReplyMsg) {
        self.all = Some(msg);
    }

    pub(super) fn clone_any(&self) -> Option<NudgeReplyMsg> {
        self.all.clone()
    }
}
