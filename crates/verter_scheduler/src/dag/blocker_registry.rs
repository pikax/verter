//! Artifact blocker-dep registry — child module of `dag`.
//!
//! Late-discovered Artifact prerequisite blockers ride here when
//! they are discovered AFTER the owner's Analysis identity has
//! already dispatched (or already completed). The DAG owns the
//! registry: writes and reads serialize through the DAG mutex, so
//! the producer (`register_resolved_deps`), the Artifact-admission
//! consumer (`admit_artifact_with_blockers`), and the lifecycle
//! sweeps (supersede / remove / Artifact completion) cannot
//! interleave with each other.
//!
//! The storage itself stays on [`SchedulerDag`] (see
//! `artifact_blocker_deps`) so the existing race-safety
//! contract — every read/write happens under the DAG mutex —
//! is preserved structurally. This module owns the typed API
//! that wraps the underlying `FxHashMap`.
//!
//! Each registry slot carries a [`PendingBlockerSet`] — the pair
//! of still-gating `DepKey`s and any [`FailedDepRecord`]s for
//! producers that terminalized BEFORE the Artifact admission. The
//! pair travels through one drain point so the Artifact admission
//! re-classifies live deps AND attaches failure markers in one
//! atomic step.

use std::sync::Arc;

use super::{DepKey, PendingBlockerSet, SchedulerDag};

impl SchedulerDag {
    fn add_analysis_blocker_demand(&mut self, set: &PendingBlockerSet) {
        for dep in &set.deps {
            *self.analysis_blocker_demand.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    fn remove_analysis_blocker_demand(&mut self, set: &PendingBlockerSet) {
        for dep in &set.deps {
            self.remove_analysis_blocker_dep(dep);
        }
    }

    fn remove_analysis_blocker_dep(&mut self, dep: &DepKey) {
        let Some(count) = self.analysis_blocker_demand.get_mut(dep) else {
            verter_debug_assert!(false, "blocker demand reverse index lost a live dependency");
            return;
        };
        if *count <= 1 {
            self.analysis_blocker_demand.remove(dep);
        } else {
            *count -= 1;
        }
    }

    /// Store `set` under `key`, indexing its owner generation, the
    /// canonicals it references and its analysis demand in the same step.
    /// `set` must be non-empty and `key` vacant.
    fn store_blocker_entry(&mut self, key: (Arc<str>, u64), set: PendingBlockerSet) {
        verter_debug_assert!(!set.is_empty(), "an empty blocker set is stored as absence");
        self.add_analysis_blocker_demand(&set);
        self.canonical_index.add_blocker_owner(&key.0, key.1);
        self.canonical_index.add_blocker_refs(&key, &set);
        let previous = self.artifact_blocker_deps.insert(key, set);
        verter_debug_assert!(previous.is_none(), "a blocker entry is stored once");
    }

    /// Remove the entry under `key`, dropping every index the store added.
    fn take_blocker_entry(&mut self, key: &(Arc<str>, u64)) -> Option<PendingBlockerSet> {
        let set = self.artifact_blocker_deps.remove(key)?;
        self.remove_analysis_blocker_demand(&set);
        self.canonical_index.remove_blocker_owner(&key.0, key.1);
        self.canonical_index.remove_blocker_refs(key, &set);
        Some(set)
    }

    /// Record a late blocker set for `(owner, generation)`. Replaces
    /// any prior entry — a second `record` for the same key is treated
    /// as the new authoritative blocker set, not an append. An empty
    /// `set` (no deps AND no failed records) drops the entry entirely
    /// (no entry is ever stored as a fully-empty
    /// [`PendingBlockerSet`]).
    pub(crate) fn record_artifact_blockers(
        &mut self,
        owner: &Arc<str>,
        generation: u64,
        set: PendingBlockerSet,
    ) {
        let key = (Arc::clone(owner), generation);
        let _ = self.take_blocker_entry(&key);
        if !set.is_empty() {
            self.store_blocker_entry(key, set);
        }
    }

    /// Drain and return the blocker set for `(owner, generation)`.
    /// Returns an empty [`PendingBlockerSet`] when no entry exists.
    /// The entry is removed in either case — callers re-attach the
    /// blockers and failure markers to their Artifact submission
    /// and the registry stays minimal. Callers MUST hold the DAG
    /// lock around the drain + submit pair to ensure the set the
    /// dispatched Artifact carries matches the registry's view at
    /// the moment of admission.
    pub(crate) fn drain_artifact_blockers(
        &mut self,
        owner: &Arc<str>,
        generation: u64,
    ) -> PendingBlockerSet {
        let key = (Arc::clone(owner), generation);
        self.take_blocker_entry(&key).unwrap_or_default()
    }

    /// Peek at the blocker set for `(owner, generation)` without
    /// draining it. Returns an empty [`PendingBlockerSet`] when no
    /// entry exists. Used by paths that need to filter the set
    /// against live DAG state before deciding whether to re-publish
    /// (drain) or drop.
    #[cfg(test)]
    pub(crate) fn peek_artifact_blockers(
        &self,
        owner: &Arc<str>,
        generation: u64,
    ) -> PendingBlockerSet {
        let key = (Arc::clone(owner), generation);
        self.artifact_blocker_deps
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    /// Clear the blocker set for `(owner, generation)`. Called when
    /// the owner is superseded (a higher generation is now live), on
    /// successful Artifact completion (all profiles done at this
    /// generation), or after an empty-blocker update (the caller now
    /// believes there are no late blockers).
    pub(crate) fn clear_artifact_blockers(&mut self, owner: &Arc<str>, generation: u64) {
        let key = (Arc::clone(owner), generation);
        let _ = self.take_blocker_entry(&key);
    }

    /// Scrub every recorded blocker entry for any `DepKey` (live or
    /// failed) that references `canonical`. Called on `remove()` so
    /// that a stale `FileStage` dep on a removed file does not pin
    /// an Artifact at another file forever. Empty entries (no live
    /// deps AND no failed records) are dropped.
    ///
    /// Visits only the entries that reference `canonical`, through the
    /// per-canonical reference index.
    pub(crate) fn scrub_artifact_blockers_referencing(&mut self, canonical: &str) {
        for key in self.canonical_index.blocker_entries_referencing(canonical) {
            let Some(mut set) = self.take_blocker_entry(&key) else {
                continue;
            };
            set.deps
                .retain(|dep| !dep_references_canonical(dep, canonical));
            set.failed
                .retain(|record| !dep_references_canonical(&record.dep_key, canonical));
            if !set.is_empty() {
                self.store_blocker_entry(key, set);
            }
        }
    }

    /// Drop every recorded blocker entry whose OWNER is `canonical`.
    /// Distinct from [`Self::scrub_artifact_blockers_referencing`],
    /// which scrubs DepKey references inside other-owner entries.
    /// Called on `remove(canonical)` before the FileNode disappears
    /// so a fresh `record_artifact_blockers(canonical, ...)` cannot
    /// race with a stale owner entry from the prior incarnation.
    ///
    /// Visits only the owner's generations through the per-canonical
    /// index.
    pub(crate) fn artifact_blocker_deps_remove_owner(&mut self, canonical: &str) {
        let Some((owner, generations)) = self.canonical_index.remove_blocker_owner_all(canonical)
        else {
            return;
        };
        for generation in generations {
            let _ = self.take_blocker_entry(&(Arc::clone(&owner), generation));
        }
    }
}

/// Whether `dep` carries `canonical` as the file-stage or artifact
/// canonical payload. CacheNode deps are never tied to a specific
/// canonical file so they are never scrubbed by canonical removal.
fn dep_references_canonical(dep: &DepKey, canonical: &str) -> bool {
    match dep {
        DepKey::FileStage { canonical: c, .. } | DepKey::Artifact { canonical: c, .. } => {
            c.as_ref() == canonical
        }
        DepKey::CacheNode { .. } => false,
    }
}
