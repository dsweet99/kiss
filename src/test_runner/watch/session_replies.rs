use std::path::{Path, PathBuf};

#[cfg(unix)]
use super::control::NudgeReplyMsg;
use super::reload::WatchLiveConfig;
#[cfg(not(unix))]
use super::session_cycle::NudgeReplyMsg;

#[derive(Clone, Default)]
pub(super) struct LastReplies {
    pub(super) repo: PathBuf,
    pub(super) ignore: Vec<String>,
    pub(super) extra: Vec<String>,
    pub(super) python_extra: Vec<String>,
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
        last.stamp_session(&live.ignore, &live.extra, &live.python_extra);
        last
    }

    pub(super) fn stamp_session(
        &mut self,
        ignore: &[String],
        extra: &[String],
        python_extra: &[String],
    ) {
        let changed =
            self.ignore != ignore || self.extra != extra || self.python_extra != python_extra;
        self.ignore = ignore.to_vec();
        self.extra = extra.to_vec();
        self.python_extra = python_extra.to_vec();
        if changed {
            self.all = None;
        }
    }

    pub(super) fn matches_args(&self, args: &crate::test_runner::RunTestCmdArgs<'_>) -> bool {
        self.ignore == args.ignore
            && self.extra == args.extra
            && self.python_extra == args.python_extra
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
