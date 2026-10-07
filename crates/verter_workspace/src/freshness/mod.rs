//! Reader-owned content-transition history.
//!
//! The workspace is the sole content authority, so it answers the
//! per-canonical freshness question every retained content-derived artifact
//! asks: "was this canonical's content transitioned after generation `G`?"
//! ([`FreshnessHistory::last_transition`]). An artifact built at `G` is
//! content-fresh only while `G >= last_transition(canonical)`.
//!
//! Two kinds of transition evidence are recorded:
//!
//! - an EXACT transition for one canonical (overlay write/clear, snapshot
//!   inject/remove, disk write/copy/delete);
//! - a SUBTREE transition for a directory prefix whose member set the
//!   engine cannot enumerate (`delete_dir_all`, watcher directory-tree
//!   recovery). It folds into every canonical under the prefix.
//!
//! ## Retirement
//!
//! Keeping every changed path forever grows with path churn. History is
//! retired into one monotone `floor`: a canonical with no retained evidence
//! answers at least `floor`, and the floor only rises past evidence it
//! retires. Every answer is therefore MONOTONE per canonical and never below
//! its true last transition: retirement can make an artifact look stale
//! sooner, never make a stale artifact look fresh.
//!
//! What keeps a live reader from being made falsely stale is reader-owned
//! evidence:
//!
//! - a [`CanonicalFreshnessLease`] (held by a retained artifact) pins its
//!   canonical's EXACT entry. A leased canonical answers from that entry,
//!   never from the floor, so unrelated retirement cannot move it. A subtree
//!   entry that retires folds its generation into every leased canonical
//!   under it first, so a leased canonical a directory event made stale
//!   stays stale.
//! - a [`ViewFreshnessLease`] (held by a request store view that clamps
//!   every answer to its captured generation) caps the floor at that
//!   generation while it lives.
//!
//! Unleased evidence is retired once enough of it is queued
//! ([`DEFAULT_RETIRE_TRIGGER`]), so the floor moves rarely and the resident
//! history is bounded by the trigger plus the evidence live readers own.
//! An unleased consumer comparing two reads for equality sees a floor rise
//! as a transition: conservative, never unsound.
//!
//! ## Lookup
//!
//! Subtree containment is indexed by ancestor: a lookup probes the
//! canonical itself and each of its `/`-delimited ancestors in the subtree
//! map, so its work is bounded by the canonical's depth, not by how many
//! directory events were ever recorded.
//!
//! The history lock is a leaf: nothing else is acquired while it is held.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use verter_debug_assert::verter_debug_assert;

#[cfg(test)]
#[path = "freshness_tests.rs"]
mod freshness_tests;

/// Unleased entries queued for retirement before a retirement pass runs.
///
/// A pass raises the floor, which every unleased canonical observes as a
/// transition; batching keeps that rare while bounding the resident history.
pub const DEFAULT_RETIRE_TRIGGER: usize = 4096;

/// Below this many slots a map's spare capacity is not worth releasing.
const SHRINK_FLOOR: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntryKind {
    Exact,
    Subtree,
}

#[derive(Debug, Clone, Copy)]
struct ExactEntry {
    generation: u64,
    /// Live [`CanonicalFreshnessLease`]s on this canonical. A leased entry
    /// is never queued for retirement and never answers from the floor.
    readers: usize,
}

#[derive(Default)]
struct HistoryState {
    exact: FxHashMap<Arc<str>, ExactEntry>,
    /// Keyed by the prefix with every trailing `/` removed, so the root
    /// `/` is the empty key — the containment rule of
    /// [`crate::path_matches_prefix`].
    subtrees: FxHashMap<Arc<str>, u64>,
    /// Retirement candidates in generation order: every UNLEASED exact
    /// entry and every subtree entry.
    queue: BTreeSet<(u64, EntryKind, Arc<str>)>,
    /// Live view leases by captured generation.
    views: BTreeMap<u64, usize>,
    floor: u64,
    /// Highest content generation the history has been told is current.
    /// The floor never passes it, so an artifact built at the current
    /// generation is never made stale by retirement.
    observed_current: u64,
}

/// Opt-in measurement of the history's actual work.
#[cfg(feature = "semantic-observe")]
#[derive(Default)]
struct ObserveCounters {
    ancestor_probes: std::sync::atomic::AtomicU64,
    retirement_passes: std::sync::atomic::AtomicU64,
    retired_entries: std::sync::atomic::AtomicU64,
}

/// Opt-in measurement snapshot ([`FreshnessHistory::observe_snapshot`]).
#[cfg(feature = "semantic-observe")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FreshnessObserveSnapshot {
    /// Subtree-map probes performed by lookups and records.
    pub ancestor_probes: u64,
    /// Retirement passes run.
    pub retirement_passes: u64,
    /// Exact and subtree entries retired.
    pub retired_entries: u64,
}

/// Current occupancy of the freshness history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FreshnessResidency {
    /// Retained exact entries, leased or not.
    pub exact_entries: usize,
    /// Exact entries pinned by at least one canonical lease.
    pub leased_entries: usize,
    /// Retained subtree entries.
    pub subtree_entries: usize,
    /// Entries queued for retirement.
    pub queued_entries: usize,
    /// Allocated slots behind the exact map.
    pub exact_capacity: usize,
    /// Allocated slots behind the subtree map.
    pub subtree_capacity: usize,
    /// Live view leases.
    pub view_leases: usize,
    /// The generation every unleased canonical with no retained evidence
    /// answers.
    pub floor: u64,
}

/// The per-workspace content-transition history. See the module docs.
pub(crate) struct FreshnessHistory {
    state: RwLock<HistoryState>,
    retire_trigger: usize,
    #[cfg(feature = "semantic-observe")]
    observe: ObserveCounters,
}

impl FreshnessHistory {
    pub(crate) fn new() -> Self {
        Self::with_retire_trigger(DEFAULT_RETIRE_TRIGGER)
    }

    pub(crate) fn with_retire_trigger(retire_trigger: usize) -> Self {
        Self {
            state: RwLock::new(HistoryState::default()),
            retire_trigger: retire_trigger.max(1),
            #[cfg(feature = "semantic-observe")]
            observe: ObserveCounters::default(),
        }
    }

    /// The generation of `canonical_id`'s most recent content transition,
    /// or a later one: `0` when nothing was ever recorded for it.
    pub(crate) fn last_transition(&self, canonical_id: &str) -> u64 {
        let key = verter_session_query::resolution::normalize_canonical_id(canonical_id);
        let state = self.state.read();
        self.last_transition_in(&state, &key)
    }

    /// Record an exact transition for `canonical_id` at `generation`.
    ///
    /// The recorded answer is strictly newer than the previous answer for
    /// this canonical, every time: a byte-less transition may already have
    /// moved it to or past `generation`, and a consumer refused at that
    /// value must not be handed it back. `current` is the live content
    /// generation.
    pub(crate) fn record_exact(&self, canonical_id: &str, generation: u64, current: u64) {
        let key: Arc<str> =
            verter_session_query::resolution::normalize_canonical_id(canonical_id).into();
        let mut state = self.state.write();
        state.observed_current = state.observed_current.max(current);
        let previous = self.last_transition_in(&state, &key);
        let next = if generation > previous {
            generation
        } else {
            previous + 1
        };
        let state = &mut *state;
        match state.exact.get_mut(&key) {
            Some(entry) => {
                if entry.readers == 0 {
                    state
                        .queue
                        .remove(&(entry.generation, EntryKind::Exact, Arc::clone(&key)));
                    state
                        .queue
                        .insert((next, EntryKind::Exact, Arc::clone(&key)));
                }
                entry.generation = next;
            }
            None => {
                state
                    .queue
                    .insert((next, EntryKind::Exact, Arc::clone(&key)));
                state.exact.insert(
                    key,
                    ExactEntry {
                        generation: next,
                        readers: 0,
                    },
                );
            }
        }
        self.maybe_retire(state);
    }

    /// Record a subtree transition: every canonical under `prefix`
    /// (inclusive) transitioned at `generation`. `current` is the live
    /// content generation.
    pub(crate) fn record_subtree(&self, prefix: &str, generation: u64, current: u64) {
        let key = subtree_key(prefix);
        let mut state = self.state.write();
        let state = &mut *state;
        state.observed_current = state.observed_current.max(current);
        let previous = state.subtrees.get(&key).copied();
        let next = previous.map_or(generation, |previous| previous.max(generation));
        if let Some(previous) = previous {
            state
                .queue
                .remove(&(previous, EntryKind::Subtree, Arc::clone(&key)));
        }
        state
            .queue
            .insert((next, EntryKind::Subtree, Arc::clone(&key)));
        state.subtrees.insert(key, next);
        self.maybe_retire(state);
    }

    pub(crate) fn residency(&self) -> FreshnessResidency {
        let state = self.state.read();
        FreshnessResidency {
            exact_entries: state.exact.len(),
            leased_entries: state
                .exact
                .values()
                .filter(|entry| entry.readers > 0)
                .count(),
            subtree_entries: state.subtrees.len(),
            queued_entries: state.queue.len(),
            exact_capacity: state.exact.capacity(),
            subtree_capacity: state.subtrees.capacity(),
            view_leases: state.views.values().sum(),
            floor: state.floor,
        }
    }

    #[cfg(feature = "semantic-observe")]
    pub(crate) fn observe_snapshot(&self) -> FreshnessObserveSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        FreshnessObserveSnapshot {
            ancestor_probes: self.observe.ancestor_probes.load(Relaxed),
            retirement_passes: self.observe.retirement_passes.load(Relaxed),
            retired_entries: self.observe.retired_entries.load(Relaxed),
        }
    }

    fn last_transition_in(&self, state: &HistoryState, key: &str) -> u64 {
        let (exact, floor) = match state.exact.get(key) {
            // A leased canonical answers from its own entry: unrelated
            // retirement never moves it.
            Some(entry) if entry.readers > 0 => (entry.generation, 0),
            Some(entry) => (entry.generation, state.floor),
            None => (0, state.floor),
        };
        exact.max(floor).max(self.subtree_max(state, key))
    }

    /// The newest subtree transition containing `key`.
    fn subtree_max(&self, state: &HistoryState, key: &str) -> u64 {
        if state.subtrees.is_empty() {
            return 0;
        }
        let mut newest = 0;
        for candidate in containing_prefixes(key) {
            #[cfg(feature = "semantic-observe")]
            self.observe
                .ancestor_probes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Some(generation) = state.subtrees.get(candidate) {
                newest = newest.max(*generation);
            }
        }
        newest
    }

    fn acquire_canonical(&self, key: &Arc<str>) {
        let mut state = self.state.write();
        let floor = state.floor;
        let state = &mut *state;
        match state.exact.get_mut(key) {
            Some(entry) => {
                if entry.readers == 0 {
                    state
                        .queue
                        .remove(&(entry.generation, EntryKind::Exact, Arc::clone(key)));
                    // Leaving the floor out of the answer must not lower
                    // it: the entry absorbs the floor it was answering at.
                    entry.generation = entry.generation.max(floor);
                }
                entry.readers += 1;
            }
            None => {
                state.exact.insert(
                    Arc::clone(key),
                    ExactEntry {
                        generation: floor,
                        readers: 1,
                    },
                );
            }
        }
    }

    fn release_canonical(&self, key: &Arc<str>) {
        let mut state = self.state.write();
        let state = &mut *state;
        let entry = state.exact.get_mut(key);
        verter_debug_assert!(
            entry.is_some(),
            "canonical freshness lease released without an entry"
        );
        let Some(entry) = entry else {
            return;
        };
        verter_debug_assert!(
            entry.readers > 0,
            "canonical freshness lease double-released"
        );
        entry.readers = entry.readers.saturating_sub(1);
        if entry.readers > 0 {
            return;
        }
        if entry.generation <= state.floor {
            // Answers exactly what the floor answers: nothing to keep.
            state.exact.remove(key);
            Self::shrink(&mut state.exact);
        } else {
            state
                .queue
                .insert((entry.generation, EntryKind::Exact, Arc::clone(key)));
            self.maybe_retire(state);
        }
    }

    fn acquire_view(&self, generation: u64) {
        let mut state = self.state.write();
        state.observed_current = state.observed_current.max(generation);
        *state.views.entry(generation).or_insert(0) += 1;
    }

    fn release_view(&self, generation: u64) {
        let mut state = self.state.write();
        let state = &mut *state;
        let count = state.views.get(&generation).copied();
        verter_debug_assert!(
            count.is_some(),
            "view freshness lease released without a record"
        );
        match count {
            Some(count) if count > 1 => {
                state.views.insert(generation, count - 1);
            }
            Some(_) => {
                state.views.remove(&generation);
            }
            None => {}
        }
        self.maybe_retire(state);
    }

    fn maybe_retire(&self, state: &mut HistoryState) {
        if state.queue.len() >= self.retire_trigger {
            self.retire(state);
        }
    }

    /// Raise the floor as far as the live views and the current generation
    /// allow, and drop every queued entry it covers.
    fn retire(&self, state: &mut HistoryState) {
        let view_cap = state.views.keys().next().copied().unwrap_or(u64::MAX);
        state.floor = state.floor.max(state.observed_current.min(view_cap));
        #[cfg(feature = "semantic-observe")]
        self.observe
            .retirement_passes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let mut retired_subtrees: FxHashMap<Arc<str>, u64> = FxHashMap::default();
        while let Some((generation, _, _)) = state.queue.first() {
            if *generation > state.floor {
                break;
            }
            let Some((generation, kind, key)) = state.queue.pop_first() else {
                break;
            };
            #[cfg(feature = "semantic-observe")]
            self.observe
                .retired_entries
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match kind {
                EntryKind::Exact => {
                    let removed = state.exact.remove(&key);
                    verter_debug_assert!(
                        removed.is_some_and(|entry| entry.readers == 0),
                        "queued exact freshness entry must be unleased"
                    );
                }
                EntryKind::Subtree => {
                    state.subtrees.remove(&key);
                    retired_subtrees.insert(key, generation);
                }
            }
        }

        if !retired_subtrees.is_empty() {
            // Unleased canonicals under a retired subtree answer from the
            // floor, which now covers it. Leased ones never consult the
            // floor, so the subtree's generation folds into their entries.
            for (key, entry) in state.exact.iter_mut() {
                if entry.readers == 0 {
                    continue;
                }
                for candidate in containing_prefixes(key) {
                    #[cfg(feature = "semantic-observe")]
                    self.observe
                        .ancestor_probes
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if let Some(generation) = retired_subtrees.get(candidate) {
                        entry.generation = entry.generation.max(*generation);
                    }
                }
            }
        }

        Self::shrink(&mut state.exact);
        Self::shrink(&mut state.subtrees);
    }

    fn shrink<V>(map: &mut FxHashMap<Arc<str>, V>) {
        if map.capacity() > SHRINK_FLOOR && map.len() * 4 < map.capacity() {
            map.shrink_to((map.len() * 2).max(SHRINK_FLOOR));
        }
    }
}

/// Subtree key for `prefix`: normalized, with every trailing `/` removed.
fn subtree_key(prefix: &str) -> Arc<str> {
    let mut key = verter_session_query::resolution::normalize_canonical_id(prefix);
    while key.ends_with('/') {
        key.pop();
    }
    key.into()
}

/// Every subtree key whose prefix contains `key` under
/// [`crate::path_matches_prefix`]: `key` itself and each part of it that
/// ends right before a `/`.
fn containing_prefixes(key: &str) -> impl Iterator<Item = &str> {
    std::iter::once(key).chain(
        key.bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'/')
            .map(move |(index, _)| &key[..index]),
    )
}

/// Shared handle a reader uses to own freshness evidence in one workspace's
/// history.
#[derive(Clone)]
pub struct FreshnessReaders(Arc<FreshnessHistory>);

impl std::fmt::Debug for FreshnessReaders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FreshnessReaders").finish_non_exhaustive()
    }
}

impl FreshnessReaders {
    pub(crate) fn new(history: Arc<FreshnessHistory>) -> Self {
        Self(history)
    }

    /// Pin `canonical_id`'s evidence for a retained artifact built from its
    /// content. While the lease lives the canonical answers from its own
    /// retained entry, so retiring unrelated history never moves it.
    #[must_use]
    pub fn lease_canonical(&self, canonical_id: &str) -> CanonicalFreshnessLease {
        let key: Arc<str> =
            verter_session_query::resolution::normalize_canonical_id(canonical_id).into();
        self.0.acquire_canonical(&key);
        CanonicalFreshnessLease {
            history: Arc::clone(&self.0),
            key,
        }
    }

    /// Opt-in measurement of the history's actual work.
    #[cfg(feature = "semantic-observe")]
    #[must_use]
    pub fn observe_snapshot(&self) -> FreshnessObserveSnapshot {
        self.0.observe_snapshot()
    }

    /// Current occupancy of the history.
    #[must_use]
    pub fn residency(&self) -> FreshnessResidency {
        self.0.residency()
    }

    /// Cap retirement at `content_generation` for a reader that clamps its
    /// answers to that captured generation.
    #[must_use]
    pub fn lease_view(&self, content_generation: u64) -> ViewFreshnessLease {
        self.0.acquire_view(content_generation);
        ViewFreshnessLease {
            history: Arc::clone(&self.0),
            generation: content_generation,
        }
    }
}

/// A retained artifact's ownership of its canonical's freshness evidence.
/// Released on drop.
pub struct CanonicalFreshnessLease {
    history: Arc<FreshnessHistory>,
    key: Arc<str>,
}

impl std::fmt::Debug for CanonicalFreshnessLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CanonicalFreshnessLease")
            .field(&self.key)
            .finish()
    }
}

impl Drop for CanonicalFreshnessLease {
    fn drop(&mut self) {
        self.history.release_canonical(&self.key);
    }
}

/// A view's cap on retirement at its captured content generation. Released
/// on drop.
pub struct ViewFreshnessLease {
    history: Arc<FreshnessHistory>,
    generation: u64,
}

impl std::fmt::Debug for ViewFreshnessLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ViewFreshnessLease")
            .field(&self.generation)
            .finish()
    }
}

impl Drop for ViewFreshnessLease {
    fn drop(&mut self) {
        self.history.release_view(self.generation);
    }
}
