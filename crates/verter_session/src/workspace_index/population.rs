//! The host's index population, maintained from committed mutations.
//!
//! The scheduler's source directory publishes every source transition under
//! one lock and journals the canonicals it touched, so a request learns what
//! changed since the population was last read in O(changes). The one
//! membership fact the scheduler does not publish, a canonical's host-side
//! eviction flag, has its own journal here. Neither feed enumerates tracked
//! files: a walk reads the directory once, when it has no population or the
//! journal no longer reaches its cursor, and reconciles changes after that.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use verter_identity::identity::{SourceRevision, SourceUnitId};
use verter_language::FileLanguage;

use super::facts::IndexSurface;
use super::identity::{ProfileClaim, ScopeId};

/// The surfaces a walk still owes one file. A source change owes every
/// surface; a walk clears the ones it published for the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PendingSurfaces(u8);

impl PendingSurfaces {
    const fn bit(surface: IndexSurface) -> u8 {
        1 << (surface.facet() - 1)
    }

    /// Every surface.
    pub(super) fn all() -> Self {
        let mut all = Self::default();
        for surface in IndexSurface::ALL {
            all.insert(surface);
        }
        all
    }

    /// No surface.
    pub(super) fn none() -> Self {
        Self::default()
    }

    pub(super) fn contains(self, surface: IndexSurface) -> bool {
        self.0 & Self::bit(surface) != 0
    }

    pub(super) fn insert(&mut self, surface: IndexSurface) {
        self.0 |= Self::bit(surface);
    }

    /// Drop every surface in `cleared`.
    pub(super) fn remove_all(&mut self, cleared: Self) {
        self.0 &= !cleared.0;
    }
}

/// Entries the membership journal retains before a reader falls back to a
/// whole read. Eviction flags move rarely, so a small bound suffices.
const MEMBERSHIP_JOURNAL_CAPACITY: usize = 4096;

/// Where the population was last read to. Any difference means a walk has
/// mutations to reconcile; equality means the population is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PopulationCursor {
    /// The source directory epoch.
    pub source_epoch: u64,
    /// The membership journal sequence.
    pub membership_seq: u64,
    /// The project generation; a change resets every slot's revision.
    pub project_generation: u64,
}

/// One tracked file the host can read, as the population last saw it.
#[derive(Debug, Clone)]
pub(super) struct PopulationEntry {
    pub unit: SourceUnitId,
    pub language: FileLanguage,
    pub claim: ProfileClaim,
    pub whole_hash: [u8; 16],
    pub generation: u64,
    pub revision: SourceRevision,
    /// The surfaces a walk of a settled scope must still visit this file for:
    /// its source changed since a walk last published them, or a walk could
    /// not certify them.
    pub pending: PendingSurfaces,
}

/// The host's population and what each walk still owes it.
#[derive(Debug, Default)]
pub(super) struct HostPopulation {
    pub cursor: Option<PopulationCursor>,
    /// Tracked files with a readable source, in canonical order.
    pub files: BTreeMap<Arc<str>, PopulationEntry>,
    /// Tracked files whose source the host cannot read, with their content
    /// hash. They block every surface until they read.
    pub unreadable: BTreeMap<Arc<str>, [u8; 16]>,
    /// Scopes a walk has visited every file for since this population was
    /// captured. A later walk of a settled scope visits pending files only.
    pub settled: BTreeSet<ScopeId>,
    /// Units whose slots of every surface must leave the store: the file is
    /// gone, unreadable, or claims another carrier now. Keyed by locator and
    /// the claim the slots were written under, and owed until a publication
    /// carrying the retirement commits, however many walks are cancelled or
    /// refused in between.
    pub retirements: BTreeMap<(Arc<str>, ProfileClaim), SourceUnitId>,
}

impl HostPopulation {
    /// Forget everything: the next walk reads the directory whole.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Host-side membership transitions the scheduler does not publish: the
/// canonicals whose eviction flag moved, in order.
#[derive(Debug, Default)]
pub(crate) struct MembershipJournal {
    inner: Mutex<MembershipState>,
}

#[derive(Debug, Default)]
struct MembershipState {
    seq: u64,
    entries: VecDeque<(u64, Arc<str>)>,
    /// Every entry with a sequence above this is retained.
    complete_above: u64,
}

impl MembershipJournal {
    /// Record that `canonical`'s membership may have changed.
    pub(crate) fn record(&self, canonical: &str) {
        let mut state = self.inner.lock();
        state.seq += 1;
        let seq = state.seq;
        state.entries.push_back((seq, Arc::from(canonical)));
        while state.entries.len() > MEMBERSHIP_JOURNAL_CAPACITY {
            if let Some((dropped, _)) = state.entries.pop_front() {
                state.complete_above = state.complete_above.max(dropped);
            }
        }
    }

    /// The current sequence.
    pub(super) fn current(&self) -> u64 {
        self.inner.lock().seq
    }

    /// The canonicals recorded after `seq`, with the sequence the answer is
    /// current at; `None` when the journal no longer reaches `seq`.
    pub(super) fn changes_since(&self, seq: u64) -> (u64, Option<Vec<Arc<str>>>) {
        let state = self.inner.lock();
        let current = state.seq;
        if seq >= current {
            return (current, Some(Vec::new()));
        }
        if seq < state.complete_above {
            return (current, None);
        }
        let changed: BTreeSet<Arc<str>> = state
            .entries
            .iter()
            .rev()
            .take_while(|(at, _)| *at > seq)
            .map(|(_, canonical)| Arc::clone(canonical))
            .collect();
        (current, Some(changed.into_iter().collect()))
    }
}
