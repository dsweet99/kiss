use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static PYTEST: AtomicUsize = AtomicUsize::new(0);
static CARGO: AtomicUsize = AtomicUsize::new(0);
static NEXTEST: AtomicUsize = AtomicUsize::new(0);
static LLVM_EXPORT: AtomicUsize = AtomicUsize::new(0);

pub trait SubprocessObserver: Send + Sync {
    fn record_pytest(&self);
    fn record_cargo_nextest(&self);
    fn record_llvm_export(&self, count: usize);
    fn snapshot(&self) -> SubprocessObserverSnapshot;
}

struct AtomicSubprocessObserver;

impl SubprocessObserver for AtomicSubprocessObserver {
    fn record_pytest(&self) {
        PYTEST.fetch_add(1, Ordering::Relaxed);
    }

    fn record_cargo_nextest(&self) {
        CARGO.fetch_add(1, Ordering::Relaxed);
        NEXTEST.fetch_add(1, Ordering::Relaxed);
    }

    fn record_llvm_export(&self, count: usize) {
        LLVM_EXPORT.fetch_add(count, Ordering::Relaxed);
    }

    fn snapshot(&self) -> SubprocessObserverSnapshot {
        SubprocessObserverSnapshot {
            pytest_invocations: PYTEST.load(Ordering::Relaxed),
            cargo_invocations: CARGO.load(Ordering::Relaxed),
            nextest_invocations: NEXTEST.load(Ordering::Relaxed),
            llvm_export_invocations: LLVM_EXPORT.load(Ordering::Relaxed),
        }
    }
}

fn installed() -> &'static Mutex<Arc<dyn SubprocessObserver>> {
    static SLOT: OnceLock<Mutex<Arc<dyn SubprocessObserver>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Arc::new(AtomicSubprocessObserver)))
}

fn current() -> Arc<dyn SubprocessObserver> {
    installed()
        .lock()
        .expect("subprocess observer")
        .clone()
}

pub fn bind_subprocess_observer(observer: Arc<dyn SubprocessObserver>) {
    *installed().lock().expect("subprocess observer") = observer;
}

pub fn record_pytest_invocation() {
    current().record_pytest();
}

pub fn record_cargo_nextest_invocation() {
    current().record_cargo_nextest();
}

pub fn record_llvm_export_invocations(count: usize) {
    current().record_llvm_export(count);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubprocessObserverSnapshot {
    pub pytest_invocations: usize,
    pub cargo_invocations: usize,
    pub nextest_invocations: usize,
    pub llvm_export_invocations: usize,
}

pub fn reset_subprocess_observer() {
    PYTEST.store(0, Ordering::Relaxed);
    CARGO.store(0, Ordering::Relaxed);
    NEXTEST.store(0, Ordering::Relaxed);
    LLVM_EXPORT.store(0, Ordering::Relaxed);
    bind_subprocess_observer(Arc::new(AtomicSubprocessObserver));
}

pub fn subprocess_observer_snapshot() -> SubprocessObserverSnapshot {
    current().snapshot()
}
