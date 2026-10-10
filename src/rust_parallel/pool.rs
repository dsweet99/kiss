use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use crate::code_roles::{AtomInterner, CfgPred, RoleBuildError, WalkOutput, walk_file};
use crate::rust_include::canonical_path;
use crate::rust_parsing::{ParsedRustFile, parse_rust_file};
use crate::shared_helpers::host_cpu_count;

type FileJob = Box<dyn FnOnce(&[ParsedRustFile]) + Send>;
type ParseAck = Receiver<Result<(), ParseFail>>;
type WorkerSet = (Vec<Sender<Job>>, Vec<JoinHandle<()>>, Vec<ParseAck>);
type SlotIndex = (Vec<(usize, usize)>, HashMap<PathBuf, (usize, usize)>);
type MapReplies<T> = Vec<Receiver<Vec<(usize, T)>>>;

enum Job {
    Parse(Vec<(usize, PathBuf)>, Sender<Result<(), ParseFail>>),
    Call(FileJob),
    Stop(Sender<Vec<(PathBuf, String)>>),
}

struct ParseFail {
    global: usize,
    err: RoleBuildError,
}

pub(super) struct Pool {
    txs: Vec<Sender<Job>>,
    joins: Vec<JoinHandle<()>>,
    slots: Vec<(usize, usize)>,
    loc: HashMap<PathBuf, (usize, usize)>,
}

impl Pool {
    pub(super) fn open(files: &[PathBuf]) -> Result<Self, RoleBuildError> {
        let bins = assign_bins(files);
        let (txs, joins, acks) = spawn_workers(&bins)?;
        wait_for_parses(&acks, &bins, files)?;
        let (slots, loc) = index_bins(bins);
        Ok(Self {
            txs,
            joins,
            slots,
            loc,
        })
    }

    pub(super) fn walk(
        &self,
        path: &Path,
        pred: &CfgPred,
        allow: bool,
        atoms: &mut AtomInterner,
    ) -> Option<Result<WalkOutput, RoleBuildError>> {
        let (worker, local) = *self.loc.get(path)?;
        let (tx, rx) = mpsc::sync_channel(1);
        let pred = pred.clone();
        let path_buf = path.to_path_buf();
        let atoms_in = std::mem::replace(atoms, AtomInterner::new());
        let sent = self.txs[worker]
            .send(Job::Call(Box::new(move |files| {
                let mut atoms = atoms_in;
                let result = walk_file(&path_buf, &files[local].ast, &pred, allow, &mut atoms);
                let _ = tx.send((result, atoms));
            })))
            .is_ok();
        if !sent {
            *atoms = AtomInterner::new();
            return Some(Err(worker_lost(path)));
        }
        match rx.recv() {
            Ok((result, atoms_out)) => {
                *atoms = atoms_out;
                Some(result)
            }
            Err(_) => {
                *atoms = AtomInterner::new();
                Some(Err(worker_lost(path)))
            }
        }
    }

    pub(super) fn map<T: Send + 'static>(
        &self,
        f: impl Fn(&ParsedRustFile) -> T + Send + Sync + 'static,
    ) -> Result<Vec<T>, RoleBuildError> {
        let f = Arc::new(f);
        let groups = worker_groups(&self.slots);
        let replies = send_maps(&self.txs, &f, groups)?;
        let mut tagged = Vec::new();
        for rx in replies {
            tagged.extend(rx.recv().map_err(|_| worker_lost(Path::new(".")))?);
        }
        tagged.sort_by_key(|(global, _)| *global);
        Ok(tagged.into_iter().map(|(_, value)| value).collect())
    }
}

fn send_maps<T, F>(
    txs: &[Sender<Job>],
    f: &Arc<F>,
    groups: Vec<(usize, Vec<(usize, usize)>)>,
) -> Result<MapReplies<T>, RoleBuildError>
where
    T: Send + 'static,
    F: Fn(&ParsedRustFile) -> T + Send + Sync + 'static,
{
    let mut replies = Vec::new();
    for (worker, locals) in groups {
        let (tx, rx) = mpsc::channel();
        let f = Arc::clone(f);
        txs[worker]
            .send(Job::Call(Box::new(move |files| {
                let mut out = Vec::with_capacity(locals.len());
                for (global, local) in locals {
                    out.push((global, f(&files[local])));
                }
                let _ = tx.send(out);
            })))
            .map_err(|_| worker_lost(Path::new(".")))?;
        replies.push(rx);
    }
    Ok(replies)
}

impl Drop for Pool {
    fn drop(&mut self) {
        let mut acks = Vec::new();
        for tx in &self.txs {
            let (ack_tx, ack_rx) = mpsc::channel();
            if tx.send(Job::Stop(ack_tx)).is_ok() {
                acks.push(ack_rx);
            }
        }
        for ack in acks {
            let _ = ack.recv();
        }
        for join in self.joins.drain(..) {
            let _ = join.join();
        }
    }
}

fn spawn_workers(
    bins: &[Vec<(usize, PathBuf)>],
) -> Result<WorkerSet, RoleBuildError> {
    let mut txs = Vec::new();
    let mut joins = Vec::new();
    let mut acks = Vec::new();
    for bin in bins {
        let (tx, rx) = mpsc::channel();
        let (ack_tx, ack_rx) = mpsc::channel();
        tx.send(Job::Parse(bin.clone(), ack_tx))
            .map_err(|_| worker_lost(Path::new(".")))?;
        joins.push(std::thread::spawn(move || worker_loop(rx)));
        txs.push(tx);
        acks.push(ack_rx);
    }
    Ok((txs, joins, acks))
}

fn wait_for_parses(
    acks: &[Receiver<Result<(), ParseFail>>],
    bins: &[Vec<(usize, PathBuf)>],
    files: &[PathBuf],
) -> Result<(), RoleBuildError> {
    let mut fails = Vec::new();
    for (worker, ack_rx) in acks.iter().enumerate() {
        match ack_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(fail)) => fails.push(fail),
            Err(_) => fails.push(ParseFail {
                global: bins
                    .get(worker)
                    .and_then(|bin| bin.first())
                    .map(|(global, _)| *global)
                    .unwrap_or(0),
                err: worker_lost(
                    files
                        .first()
                        .map(PathBuf::as_path)
                        .unwrap_or(Path::new(".")),
                ),
            }),
        }
    }
    match fails.into_iter().min_by_key(|fail| fail.global) {
        Some(fail) => Err(fail.err),
        None => Ok(()),
    }
}

fn index_bins(
    bins: Vec<Vec<(usize, PathBuf)>>,
) -> SlotIndex {
    let total = bins.iter().map(|bin| bin.len()).sum();
    let mut slots = vec![(0, 0); total];
    let mut loc = HashMap::new();
    for (worker, bin) in bins.into_iter().enumerate() {
        for (local, (global, path)) in bin.into_iter().enumerate() {
            slots[global] = (worker, local);
            loc.insert(canonical_path(&path), (worker, local));
        }
    }
    (slots, loc)
}

fn worker_groups(slots: &[(usize, usize)]) -> Vec<(usize, Vec<(usize, usize)>)> {
    let mut groups: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
    for (global, (worker, local)) in slots.iter().copied().enumerate() {
        if let Some(group) = groups.iter_mut().find(|(id, _)| *id == worker) {
            group.1.push((global, local));
        } else {
            groups.push((worker, vec![(global, local)]));
        }
    }
    groups
}

fn worker_loop(rx: Receiver<Job>) {
    let mut files: Vec<ParsedRustFile> = Vec::new();
    while let Ok(job) = rx.recv() {
        match job {
            Job::Parse(batch, ack) => parse_batch(&mut files, batch, ack),
            Job::Call(job) => job(&files),
            Job::Stop(ack) => {
                let out = std::mem::take(&mut files)
                    .into_iter()
                    .map(|parsed| (parsed.path, parsed.source))
                    .collect();
                let _ = ack.send(out);
                break;
            }
        }
    }
}

fn parse_batch(
    files: &mut Vec<ParsedRustFile>,
    batch: Vec<(usize, PathBuf)>,
    ack: Sender<Result<(), ParseFail>>,
) {
    files.clear();
    for (global, path) in batch {
        match parse_rust_file(&path) {
            Ok(parsed) => files.push(parsed),
            Err(err) => {
                files.clear();
                let _ = ack.send(Err(ParseFail {
                    global,
                    err: RoleBuildError::RustParse {
                        path,
                        message: err.to_string(),
                    },
                }));
                return;
            }
        }
    }
    let _ = ack.send(Ok(()));
}

fn assign_bins(files: &[PathBuf]) -> Vec<Vec<(usize, PathBuf)>> {
    let shards = host_cpu_count(8).clamp(1, files.len());
    let mut weighted: Vec<(u64, usize, PathBuf)> = files
        .iter()
        .enumerate()
        .map(|(global, path)| {
            let bytes = std::fs::metadata(path)
                .map(|meta| meta.len())
                .unwrap_or(1)
                .max(1);
            (bytes, global, path.clone())
        })
        .collect();
    weighted.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut bins = vec![Vec::new(); shards];
    let mut loads = vec![0_u64; shards];
    for (bytes, global, path) in weighted {
        let idx = loads
            .iter()
            .enumerate()
            .min_by_key(|(i, load)| (*load, *i))
            .map(|(i, _)| i)
            .unwrap_or(0);
        loads[idx] = loads[idx].saturating_add(bytes);
        bins[idx].push((global, path));
    }
    bins.into_iter().filter(|bin| !bin.is_empty()).collect()
}

fn worker_lost(path: &Path) -> RoleBuildError {
    RoleBuildError::RustParse {
        path: path.to_path_buf(),
        message: "rust parse worker stopped".to_string(),
    }
}
