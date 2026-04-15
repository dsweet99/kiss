use super::extraction::CodeChunk;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;

pub(super) struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    pub(super) fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    pub(super) fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    pub(super) fn union(&mut self, x: usize, y: usize) {
        let (px, py) = (self.find(x), self.find(y));
        if px != py {
            self.parent[px] = py;
        }
    }
}

#[derive(Debug, Clone)]
pub struct DuplicateCluster {
    pub chunks: Vec<CodeChunk>,
    pub avg_similarity: f64,
}

type ChunkKey = (PathBuf, usize, usize);

pub(super) fn chunk_key(c: &CodeChunk) -> ChunkKey {
    (c.file.clone(), c.start_line, c.end_line)
}

pub(super) fn build_chunk_index(chunks: &[CodeChunk]) -> HashMap<ChunkKey, usize> {
    chunks
        .iter()
        .enumerate()
        .map(|(idx, c)| (chunk_key(c), idx))
        .collect()
}

pub(super) fn compute_cluster_similarity(
    indices: &[usize],
    pair_sims: &HashMap<(usize, usize), f64>,
) -> f64 {
    // Iterate O(k²) pairs within cluster instead of O(all_pairs) - faster for small clusters
    let mut total = 0.0;
    let mut count = 0;
    for i in 0..indices.len() {
        for j in (i + 1)..indices.len() {
            let key = (indices[i].min(indices[j]), indices[i].max(indices[j]));
            if let Some(&sim) = pair_sims.get(&key) {
                total += sim;
                count += 1;
            }
        }
    }
    if count > 0 {
        total / f64::from(count)
    } else {
        0.0
    }
}

pub(crate) fn cmp_chunk_key(a: &CodeChunk, b: &CodeChunk) -> Ordering {
    a.file
        .to_string_lossy()
        .cmp(&b.file.to_string_lossy())
        .then(a.start_line.cmp(&b.start_line))
        .then(a.end_line.cmp(&b.end_line))
        .then(a.name.cmp(&b.name))
}

pub(crate) fn min_chunk_in_cluster(cluster: &DuplicateCluster) -> Option<&CodeChunk> {
    cluster.chunks.iter().min_by(|a, b| cmp_chunk_key(a, b))
}

pub(super) fn sort_clusters_deterministic(clusters: &mut [DuplicateCluster]) {
    clusters.sort_by(|a, b| {
        b.chunks
            .len()
            .cmp(&a.chunks.len())
            .then_with(|| {
                b.avg_similarity
                    .partial_cmp(&a.avg_similarity)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| {
                // Deterministic tie-breaker: ensure stable ordering when size + avg_similarity tie.
                match (min_chunk_in_cluster(a), min_chunk_in_cluster(b)) {
                    (Some(ca), Some(cb)) => cmp_chunk_key(ca, cb),
                    _ => Ordering::Equal,
                }
            })
    });
}

pub fn cluster_duplicates(
    pairs: &[super::DuplicatePair],
    chunks: &[CodeChunk],
) -> Vec<DuplicateCluster> {
    if pairs.is_empty() || chunks.len() < 2 {
        return Vec::new();
    }
    let key_to_idx = build_chunk_index(chunks);
    let mut uf = UnionFind::new(chunks.len());
    let mut pair_similarities: HashMap<(usize, usize), f64> = HashMap::new();
    for pair in pairs {
        if let (Some(&i1), Some(&i2)) = (
            key_to_idx.get(&chunk_key(&pair.chunk1)),
            key_to_idx.get(&chunk_key(&pair.chunk2)),
        ) {
            uf.union(i1, i2);
            pair_similarities.insert((i1.min(i2), i1.max(i2)), pair.similarity);
        }
    }
    let mut clusters_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for idx in 0..chunks.len() {
        clusters_map.entry(uf.find(idx)).or_default().push(idx);
    }
    let mut clusters: Vec<DuplicateCluster> = clusters_map
        .into_values()
        .filter(|indices| indices.len() > 1)
        .map(|indices| DuplicateCluster {
            avg_similarity: compute_cluster_similarity(&indices, &pair_similarities),
            chunks: indices.into_iter().map(|i| chunks[i].clone()).collect(),
        })
        .collect();
    sort_clusters_deterministic(&mut clusters);
    clusters
}

pub(super) fn cluster_from_pairs(
    chunks: &[CodeChunk],
    good_pairs: Vec<(usize, usize, f64)>,
) -> Vec<DuplicateCluster> {
    // Cluster in serial (union-find), but store only indices + similarity.
    let mut uf = UnionFind::new(chunks.len());
    let mut pair_similarities: HashMap<(usize, usize), f64> =
        HashMap::with_capacity(good_pairs.len());
    for (i, j, sim) in good_pairs {
        uf.union(i, j);
        pair_similarities.insert((i.min(j), i.max(j)), sim);
    }

    let mut clusters_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for idx in 0..chunks.len() {
        clusters_map.entry(uf.find(idx)).or_default().push(idx);
    }

    let mut clusters: Vec<DuplicateCluster> = clusters_map
        .into_values()
        .filter(|indices| indices.len() > 1)
        .map(|indices| DuplicateCluster {
            avg_similarity: compute_cluster_similarity(&indices, &pair_similarities),
            chunks: indices.into_iter().map(|i| chunks[i].clone()).collect(),
        })
        .collect();

    sort_clusters_deterministic(&mut clusters);
    clusters
}
