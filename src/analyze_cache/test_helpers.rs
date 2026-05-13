use std::sync::Mutex;
use tempfile::TempDir;

static HOME_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct ScopedHome {
    _guard: std::sync::MutexGuard<'static, ()>,
    pub _tmp: TempDir,
    prev: Option<std::ffi::OsString>,
}

impl ScopedHome {
    pub(crate) fn new() -> Self {
        let guard = HOME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", tmp.path()) };
        Self {
            _guard: guard,
            _tmp: tmp,
            prev,
        }
    }
}

impl Drop for ScopedHome {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
