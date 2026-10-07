//! Dependency-edge store — child module of `dag`.
//!
//! One representation owns every `waiter -> dependency` gating edge. An
//! edge lives in exactly two places that change together:
//!
//! - the waiter node's `deps_remaining`, which maps each [`DepKey`] it is
//!   gated on to the edge's [`EdgeSeq`];
//! - this store, which maps each [`DepKey`] to its waiters keyed by the same
//!   [`EdgeSeq`], and indexes every gated file dependency by
//!   `(canonical, generation)`.
//!
//! The sequence number is allocated when the edge is linked, so the waiters
//! of one dependency are visited in edge-admission order (the order the
//! former per-dependency vector preserved) and a single edge is removed by
//! key instead of by scanning its siblings. A dependency is gated on whether
//! or not its producer was ever admitted: the store never consults the
//! producer node, so retirement reaches waiters of identities that never
//! became nodes through the file index alone.
//!
//! Every mutation keeps the per-dependency map and the file index in one
//! step: a dependency enters the file index when its first waiter links and
//! leaves it when its last waiter unlinks or the dependency is taken.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{DepKey, SubmissionToken};

/// Admission order of one dependency edge. Unique per [`DepEdges`] store.
///
/// Every equality and ordering comparison of two sequences is counted in
/// observed builds: removing an edge from its dependency's waiters must
/// compare sequence keys to find it, so the count measures the search a
/// removal performed — a keyed lookup compares a logarithmic number, a
/// sibling scan compares every sibling it passes.
#[derive(Clone, Copy, Debug)]
pub(in crate::dag) struct EdgeSeq(u64);

impl PartialEq for EdgeSeq {
    fn eq(&self, other: &Self) -> bool {
        count_edge_seq_comparison();
        self.0 == other.0
    }
}

impl Eq for EdgeSeq {}

impl PartialOrd for EdgeSeq {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EdgeSeq {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        count_edge_seq_comparison();
        self.0.cmp(&other.0)
    }
}

#[cfg(any(test, feature = "semantic-observe"))]
thread_local! {
    static EDGE_SEQ_COMPARISONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[inline(always)]
fn count_edge_seq_comparison() {
    #[cfg(any(test, feature = "semantic-observe"))]
    EDGE_SEQ_COMPARISONS.with(|count| count.set(count.get() + 1));
}

#[cfg(any(test, feature = "semantic-observe"))]
fn edge_seq_comparisons() -> u64 {
    EDGE_SEQ_COMPARISONS.with(std::cell::Cell::get)
}

/// Waiters gated on one dependency, in edge-admission order.
pub(in crate::dag) type DepWaiters = BTreeMap<EdgeSeq, SubmissionToken>;

#[derive(Debug, Default)]
pub(in crate::dag) struct DepEdges {
    /// Dependency → waiters gated on it. Holds only dependencies with at
    /// least one waiter.
    waiters: FxHashMap<DepKey, DepWaiters>,
    /// Canonical → generation → the gated file dependencies at that
    /// generation (every incarnation). Holds only non-empty buckets, so a
    /// generation retirement visits the retired dependencies alone.
    by_file: FxHashMap<Arc<str>, BTreeMap<u64, FxHashSet<DepKey>>>,
    next_seq: u64,
    #[cfg(any(test, feature = "semantic-observe"))]
    observations: DepEdgeObservations,
}

/// Cumulative work counters for the dependency-edge store. Attribution and
/// tests only; compiled out of default builds.
#[cfg(any(test, feature = "semantic-observe"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DepEdgeObservations {
    /// Edge-sequence keys compared while searching a dependency's waiters
    /// to remove single edges. Keyed removal compares logarithmically many
    /// per edge; a scan over the siblings compares each one it passes.
    pub unlink_keys_compared: u64,
    /// File-index entries generation retirement traversed: the canonical
    /// bucket, each generation bucket and each gated dependency key it
    /// walked, counted as reached rather than as returned.
    pub retire_entries_visited: u64,
}

/// Current occupancy of the dependency-edge store and its file index.
/// Always available; every count is derived from the resident tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DepEdgeOccupancy {
    /// Linked `waiter -> dependency` edges.
    pub edges: usize,
    /// Dependencies with at least one waiter.
    pub gated_deps: usize,
    /// Backing capacity of the dependency → waiters table.
    pub gated_deps_capacity: usize,
    /// Canonicals with at least one gated file dependency.
    pub file_canonicals: usize,
    /// Backing capacity of the canonical table of the file index.
    pub file_canonicals_capacity: usize,
    /// `(canonical, generation)` buckets of the file index.
    pub file_generations: usize,
    /// Gated file dependencies held by the file index.
    pub file_deps: usize,
    /// Backing capacity summed over the file index's per-generation
    /// dependency sets. A set keeps its capacity while any member
    /// survives, so this can exceed `file_deps`. The ordered levels of
    /// the store hold no spare backing capacity.
    pub file_deps_capacity: usize,
}

impl DepEdges {
    /// Link `waiter` to `dep`, returning the edge's sequence number. The
    /// caller records it in the waiter's `deps_remaining` in the same step.
    pub(in crate::dag) fn link(&mut self, dep: DepKey, waiter: SubmissionToken) -> EdgeSeq {
        let seq = EdgeSeq(self.next_seq);
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .expect("dependency edge sequence exhausted");
        match self.waiters.entry(dep) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().insert(seq, waiter);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                index_file_dep(&mut self.by_file, entry.key());
                entry.insert(BTreeMap::new()).insert(seq, waiter);
            }
        }
        seq
    }

    /// Remove the single edge `(dep, seq)` the waiter recorded at link time.
    pub(in crate::dag) fn unlink(&mut self, dep: &DepKey, seq: EdgeSeq) {
        let Some(waiters) = self.waiters.get_mut(dep) else {
            verter_debug_assert!(false, "a recorded dependency edge is linked");
            return;
        };
        #[cfg(any(test, feature = "semantic-observe"))]
        let compared_before = edge_seq_comparisons();
        let removed = waiters.remove(&seq);
        #[cfg(any(test, feature = "semantic-observe"))]
        {
            self.observations.unlink_keys_compared += edge_seq_comparisons() - compared_before;
        }
        verter_debug_assert!(removed.is_some(), "a recorded dependency edge is linked");
        if waiters.is_empty() {
            self.waiters.remove(dep);
            unindex_file_dep(&mut self.by_file, dep);
        }
    }

    /// Detach every waiter of `dep`, in edge-admission order. Each waiter
    /// must drop `dep` from its `deps_remaining` in the same step.
    pub(in crate::dag) fn take(&mut self, dep: &DepKey) -> Option<DepWaiters> {
        let waiters = self.waiters.remove(dep)?;
        unindex_file_dep(&mut self.by_file, dep);
        Some(waiters)
    }

    /// Waiters currently gated on `dep`, in edge-admission order.
    pub(in crate::dag) fn waiters_of(
        &self,
        dep: &DepKey,
    ) -> impl Iterator<Item = SubmissionToken> + '_ {
        self.waiters
            .get(dep)
            .into_iter()
            .flat_map(|waiters| waiters.values().copied())
    }

    /// Gated dependencies on `canonical` whose generation is strictly below
    /// `floor`, across every incarnation. Visits only those dependencies.
    pub(in crate::dag) fn gated_below(&mut self, canonical: &str, floor: u64) -> Vec<DepKey> {
        let mut retired = Vec::new();
        let Some(gens) = self.by_file.get(canonical) else {
            return retired;
        };
        #[cfg(any(test, feature = "semantic-observe"))]
        {
            self.observations.retire_entries_visited += 1;
        }
        for (_, deps) in gens.range(..floor) {
            #[cfg(any(test, feature = "semantic-observe"))]
            {
                self.observations.retire_entries_visited += 1;
            }
            for dep in deps {
                #[cfg(any(test, feature = "semantic-observe"))]
                {
                    self.observations.retire_entries_visited += 1;
                }
                retired.push(dep.clone());
            }
        }
        retired
    }

    /// Drop every edge and release the tables' backing storage. The waiter
    /// nodes are dropped by the same reset.
    pub(in crate::dag) fn clear(&mut self) {
        self.waiters = FxHashMap::default();
        self.by_file = FxHashMap::default();
    }

    /// Current occupancy of the store and its file index.
    pub(in crate::dag) fn occupancy(&self) -> DepEdgeOccupancy {
        let (file_generations, file_deps, file_deps_capacity) =
            self.by_file
                .values()
                .fold((0, 0, 0), |(generations, deps, capacity), gens| {
                    (
                        generations + gens.len(),
                        deps + gens.values().map(FxHashSet::len).sum::<usize>(),
                        capacity + gens.values().map(FxHashSet::capacity).sum::<usize>(),
                    )
                });
        DepEdgeOccupancy {
            edges: self.waiters.values().map(BTreeMap::len).sum(),
            gated_deps: self.waiters.len(),
            gated_deps_capacity: self.waiters.capacity(),
            file_canonicals: self.by_file.len(),
            file_canonicals_capacity: self.by_file.capacity(),
            file_generations,
            file_deps,
            file_deps_capacity,
        }
    }

    #[cfg(any(test, feature = "semantic-observe"))]
    pub(in crate::dag) fn observations(&self) -> DepEdgeObservations {
        self.observations
    }

    /// Every gated dependency, for index-equals-scan oracles.
    #[cfg(test)]
    pub(in crate::dag) fn gated_deps(&self) -> impl Iterator<Item = &DepKey> {
        self.waiters.keys()
    }

    /// The `(canonical, generation)` file index, flattened, for
    /// index-equals-scan oracles.
    #[cfg(test)]
    pub(in crate::dag) fn indexed_file_deps(&self) -> impl Iterator<Item = &DepKey> {
        self.by_file
            .values()
            .flat_map(|gens| gens.values().flat_map(|deps| deps.iter()))
    }
}

/// The `(canonical, generation)` a file dependency is indexed under, or
/// `None` for a cache-node dependency (never retired by file generation).
fn file_slot(dep: &DepKey) -> Option<(&Arc<str>, u64)> {
    match dep {
        DepKey::FileStage {
            canonical,
            generation,
            ..
        }
        | DepKey::Artifact {
            canonical,
            generation,
            ..
        } => Some((canonical, *generation)),
        DepKey::CacheNode { .. } => None,
    }
}

fn index_file_dep(
    by_file: &mut FxHashMap<Arc<str>, BTreeMap<u64, FxHashSet<DepKey>>>,
    dep: &DepKey,
) {
    if let Some((canonical, generation)) = file_slot(dep) {
        by_file
            .entry(Arc::clone(canonical))
            .or_default()
            .entry(generation)
            .or_default()
            .insert(dep.clone());
    }
}

fn unindex_file_dep(
    by_file: &mut FxHashMap<Arc<str>, BTreeMap<u64, FxHashSet<DepKey>>>,
    dep: &DepKey,
) {
    let Some((canonical, generation)) = file_slot(dep) else {
        return;
    };
    let Some(gens) = by_file.get_mut(canonical.as_ref()) else {
        return;
    };
    if let Some(deps) = gens.get_mut(&generation) {
        deps.remove(dep);
        if deps.is_empty() {
            gens.remove(&generation);
        }
    }
    if gens.is_empty() {
        by_file.remove(canonical.as_ref());
    }
}

#[cfg(test)]
#[path = "dep_edges_tests.rs"]
mod tests;
