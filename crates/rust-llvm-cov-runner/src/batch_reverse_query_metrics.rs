//! Process-wide reverse-query telemetry counters.

use std::sync::atomic::{AtomicU64, Ordering};

/// Why a reverse snapshot query fell back to the forward-entry scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReverseUnavailableReason {
    Schema,
    Generation,
    Revision,
    Fingerprint,
    Digest,
    Malformed,
    MissingRecord,
}

impl ReverseUnavailableReason {
    pub const ALL: [Self; 7] = [
        Self::Schema,
        Self::Generation,
        Self::Revision,
        Self::Fingerprint,
        Self::Digest,
        Self::Malformed,
        Self::MissingRecord,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schema => "schema",
            Self::Generation => "generation",
            Self::Revision => "revision",
            Self::Fingerprint => "fingerprint",
            Self::Digest => "digest",
            Self::Malformed => "malformed",
            Self::MissingRecord => "missing_record",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Schema => 0,
            Self::Generation => 1,
            Self::Revision => 2,
            Self::Fingerprint => 3,
            Self::Digest => 4,
            Self::Malformed => 5,
            Self::MissingRecord => 6,
        }
    }
}

pub static REVERSE_QUERY_HITS: AtomicU64 = AtomicU64::new(0);
static UNAVAILABLE: [AtomicU64; 7] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
/// Watermark for copying process reverse counters into batch results without double-counting.
static LAST_COPIED_HITS: AtomicU64 = AtomicU64::new(0);
static LAST_COPIED_UNAVAILABLE: [AtomicU64; 7] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReverseUnavailableCounts {
    pub schema: u64,
    pub generation: u64,
    pub revision: u64,
    pub fingerprint: u64,
    pub digest: u64,
    pub malformed: u64,
    pub missing_record: u64,
}

impl ReverseUnavailableCounts {
    pub fn total(&self) -> u64 {
        self.schema
            + self.generation
            + self.revision
            + self.fingerprint
            + self.digest
            + self.malformed
            + self.missing_record
    }

    pub fn get(&self, reason: ReverseUnavailableReason) -> u64 {
        match reason {
            ReverseUnavailableReason::Schema => self.schema,
            ReverseUnavailableReason::Generation => self.generation,
            ReverseUnavailableReason::Revision => self.revision,
            ReverseUnavailableReason::Fingerprint => self.fingerprint,
            ReverseUnavailableReason::Digest => self.digest,
            ReverseUnavailableReason::Malformed => self.malformed,
            ReverseUnavailableReason::MissingRecord => self.missing_record,
        }
    }

    pub fn add_assign(&mut self, other: &Self) {
        self.schema += other.schema;
        self.generation += other.generation;
        self.revision += other.revision;
        self.fingerprint += other.fingerprint;
        self.digest += other.digest;
        self.malformed += other.malformed;
        self.missing_record += other.missing_record;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReverseQueryCounters {
    pub hits: u64,
    pub unavailable: ReverseUnavailableCounts,
}

pub fn record_reverse_hit() {
    REVERSE_QUERY_HITS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_reverse_unavailable(reason: ReverseUnavailableReason) {
    UNAVAILABLE[reason.index()].fetch_add(1, Ordering::Relaxed);
}

pub fn snapshot_reverse_query_counters() -> ReverseQueryCounters {
    ReverseQueryCounters {
        hits: REVERSE_QUERY_HITS.load(Ordering::Relaxed),
        unavailable: ReverseUnavailableCounts {
            schema: UNAVAILABLE[0].load(Ordering::Relaxed),
            generation: UNAVAILABLE[1].load(Ordering::Relaxed),
            revision: UNAVAILABLE[2].load(Ordering::Relaxed),
            fingerprint: UNAVAILABLE[3].load(Ordering::Relaxed),
            digest: UNAVAILABLE[4].load(Ordering::Relaxed),
            malformed: UNAVAILABLE[5].load(Ordering::Relaxed),
            missing_record: UNAVAILABLE[6].load(Ordering::Relaxed),
        },
    }
}

/// Process reverse counters accumulated since the last copy into batch counters.
pub fn take_reverse_query_counters_since_last_copy() -> ReverseQueryCounters {
    let current = snapshot_reverse_query_counters();
    let prior_hits = LAST_COPIED_HITS.swap(current.hits, Ordering::Relaxed);
    let mut unavailable = ReverseUnavailableCounts::default();
    for reason in ReverseUnavailableReason::ALL {
        let idx = reason.index();
        let value = current.unavailable.get(reason);
        let prior = LAST_COPIED_UNAVAILABLE[idx].swap(value, Ordering::Relaxed);
        match reason {
            ReverseUnavailableReason::Schema => {
                unavailable.schema = value.saturating_sub(prior);
            }
            ReverseUnavailableReason::Generation => {
                unavailable.generation = value.saturating_sub(prior);
            }
            ReverseUnavailableReason::Revision => {
                unavailable.revision = value.saturating_sub(prior);
            }
            ReverseUnavailableReason::Fingerprint => {
                unavailable.fingerprint = value.saturating_sub(prior);
            }
            ReverseUnavailableReason::Digest => {
                unavailable.digest = value.saturating_sub(prior);
            }
            ReverseUnavailableReason::Malformed => {
                unavailable.malformed = value.saturating_sub(prior);
            }
            ReverseUnavailableReason::MissingRecord => {
                unavailable.missing_record = value.saturating_sub(prior);
            }
        }
    }
    ReverseQueryCounters {
        hits: current.hits.saturating_sub(prior_hits),
        unavailable,
    }
}

#[cfg(test)]
pub fn reset_reverse_query_counters_for_test() {
    REVERSE_QUERY_HITS.store(0, Ordering::Relaxed);
    LAST_COPIED_HITS.store(0, Ordering::Relaxed);
    for counter in &UNAVAILABLE {
        counter.store(0, Ordering::Relaxed);
    }
    for counter in &LAST_COPIED_UNAVAILABLE {
        counter.store(0, Ordering::Relaxed);
    }
}
