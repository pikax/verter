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
        if set.is_empty() {
            self.artifact_blocker_deps.remove(&key);
        } else {
            self.artifact_blocker_deps.insert(key, set);
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
        self.artifact_blocker_deps.remove(&key).unwrap_or_default()
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
        self.artifact_blocker_deps.remove(&key);
    }

    /// Scrub every recorded blocker entry for any `DepKey` (live or
    /// failed) that references `canonical`. Called on `remove()` so
    /// that a stale `FileStage` dep on a removed file does not pin
    /// an Artifact at another file forever. Empty entries (no live
    /// deps AND no failed records) are dropped.
    pub(crate) fn scrub_artifact_blockers_referencing(&mut self, canonical: &str) {
        self.artifact_blocker_deps.retain(|_owner, set| {
            set.deps
                .retain(|dep| !dep_references_canonical(dep, canonical));
            set.failed
                .retain(|record| !dep_references_canonical(&record.dep_key, canonical));
            !set.is_empty()
        });
    }

    /// Drop every recorded blocker entry whose OWNER is `canonical`.
    /// Distinct from [`Self::scrub_artifact_blockers_referencing`],
    /// which scrubs DepKey references inside other-owner entries.
    /// Called on `remove(canonical)` before the FileNode disappears
    /// so a fresh `record_artifact_blockers(canonical, ...)` cannot
    /// race with a stale owner entry from the prior incarnation.
    pub(crate) fn artifact_blocker_deps_remove_owner(&mut self, canonical: &str) {
        self.artifact_blocker_deps
            .retain(|(owner, _gen), _| owner.as_ref() != canonical);
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
