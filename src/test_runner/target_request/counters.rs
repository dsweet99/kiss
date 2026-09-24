use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KernelCounters {
    pub parse: u32,
    pub git: u32,
    pub index_rebuild: u32,
    pub graph: u32,
    pub snapshot_retry: u32,
    pub subprocess: u32,
}

thread_local! {
    static CURRENT: Cell<KernelCounters> = const { Cell::new(KernelCounters {
        parse: 0,
        git: 0,
        index_rebuild: 0,
        graph: 0,
        snapshot_retry: 0,
        subprocess: 0,
    }) };
}

pub(crate) fn reset() {
    CURRENT.set(KernelCounters::default());
}

fn add(update: impl FnOnce(&mut KernelCounters)) {
    CURRENT.with(|cell| {
        let mut cur = cell.get();
        update(&mut cur);
        cell.set(cur);
    });
}

pub(crate) fn add_parse() {
    add(|cur| cur.parse = cur.parse.saturating_add(1));
}

pub(crate) fn add_git() {
    add(|cur| cur.git = cur.git.saturating_add(1));
}

pub(crate) fn add_index() {
    add(|cur| cur.index_rebuild = cur.index_rebuild.saturating_add(1));
}

pub(crate) fn add_graph() {
    add(|cur| cur.graph = cur.graph.saturating_add(1));
}

pub(crate) fn add_snapshot_retry() {
    add(|cur| cur.snapshot_retry = cur.snapshot_retry.saturating_add(1));
}

pub(crate) fn add_subprocess() {
    add(|cur| cur.subprocess = cur.subprocess.saturating_add(1));
}

pub(crate) fn current() -> KernelCounters {
    CURRENT.get()
}

pub(crate) fn render_line(counters: KernelCounters) -> String {
    format!(
        "kiss test: kernel parse={} git={} index={} graph={} snapshot={} subprocess={}",
        counters.parse,
        counters.git,
        counters.index_rebuild,
        counters.graph,
        counters.snapshot_retry,
        counters.subprocess
    )
}

pub(crate) fn emit() {
    crate::test_runner::emit_test_progress(&render_line(current()));
}

#[cfg(test)]
mod counters_test {
    use super::*;

    #[test]
    fn render_line_lists_kernel_fields() {
        let line = render_line(KernelCounters {
            parse: 1,
            git: 2,
            index_rebuild: 3,
            graph: 4,
            snapshot_retry: 5,
            subprocess: 0,
        });
        assert_eq!(
            line,
            "kiss test: kernel parse=1 git=2 index=3 graph=4 snapshot=5 subprocess=0"
        );
    }

    #[test]
    fn add_graph_increments_current() {
        reset();
        add_graph();
        add_graph();
        assert_eq!(current().graph, 2);
        reset();
        assert_eq!(current().graph, 0);
    }

    #[test]
    fn add_remaining_fields_increment_current() {
        reset();
        add_parse();
        add_git();
        add_index();
        add_subprocess();
        let cur = current();
        assert_eq!(cur.parse, 1);
        assert_eq!(cur.git, 1);
        assert_eq!(cur.index_rebuild, 1);
        assert_eq!(cur.subprocess, 1);
        reset();
        assert_eq!(current(), KernelCounters::default());
    }

    #[test]
    fn continue_render_includes_subprocess() {
        reset();
        add_subprocess();
        assert!(
            render_line(current()).contains("subprocess=1"),
            "{}",
            render_line(current())
        );
    }
}
