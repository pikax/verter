//! Epoch-indexed MVCC source authority: the scheduler's immutable,
//! LEASED view of what [`Scheduler::try_get_source`] logically answers.
//!
//! [`crate::scheduler::Scheduler::nodes`] is EXECUTION state — a
//! [`crate::node::FileNode`] holds only its CURRENT `ArcSwap` snapshots,
//! and [`crate::node::FileNode::bump_generation`] makes the prior source
//! immediately unreachable. Nothing there can answer "what did this file
//! look like when my request started?", and enumerating the directory to
//! find out is an O(published files) walk on every capture.
//!
//! This module adds the missing half. An immutable root must both NAME
//! state and KEEP that state reachable: content-addressing establishes
//! identity, never lifetime and never immutable membership.
//!
//! ```text
//! SchedulerSourceRoot { visible_epoch, root_lease }
//! canonical -> version history of
//!     { epoch, incarnation, generation, Present(whole_hash) | Absent }
//! ```
//!
//! - **Capture** ([`SchedulerSourceDirectory::capture_root`]) is one
//!   mutex acquisition, one scalar read and one counter bump —
//!   independent of the number of tracked canonicals.
//! - **Write** ([`SchedulerSourceDirectory::publish_transition`]) is one
//!   epoch bump plus one appended per-canonical version. No map
//!   path-copy, no global CAS retry loop.
//! - **Read** ([`SchedulerSourceRoot::lookup`]) is one canonical lookup
//!   plus a predecessor search in that canonical's SHORT retained
//!   history.
//! - **GC** ([`SchedulerSourceDirectory::reclaim_superseded_versions`])
//!   retains only versions selected by the current root or by a live
//!   captured root — the same reachability discipline
//!   `FileArtifactStore` applies to artifact versions.
//!
//! The epoch ADDRESSES a snapshot. It is NOT a cache-validity oracle and
//! must never become one: validity stays with the by-value token
//! generations and the R26 fact signatures.
//!
//! # Atomicity
//!
//! Publication is atomic with the scheduler lifecycle transition that
//! caused it. [`SchedulerSourceDirectory::publish_transition`] runs the
//! transition (the generation bump, the source commit, the node removal)
//! and the version append under ONE hold of the publication lock, and
//! [`SchedulerSourceDirectory::capture_root`] takes the SAME lock. So a
//! capture is totally ordered against every transition: it observes the
//! node state and the root membership either both before or both after,
//! never a torn pair. A batch publishes ONE epoch covering all of its
//! changed members.
//!
//! # Lock rank
//!
//! `SchedulerDag` (outermost) > source-root publication > `nodes` /
//! `versions` DashMap shards (innermost). A publication may take a
//! DashMap shard; nothing takes the publication lock while holding one,
//! and nothing takes the DAG lock while holding the publication lock.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;

/// Number of superseding publications the directory may accumulate
/// before it self-triggers a reclamation sweep.
///
/// Supersession is LOGICAL: the prior version leaves the current root's
/// answer but stays retained for every root that still addresses it.
/// Without an amortised sweep an edit loop would retain one version per
/// keystroke forever, so the directory reclaims on its own schedule
/// rather than waiting for an external request that may never arrive.
const RECLAIM_TRIGGER_SUPERSESSIONS: u64 = 64;

/// The terminal publication epoch.
///
/// The epoch counter is monotonic and never wraps: reaching this value
/// EXHAUSTS the epoch line. A wrap would invert visibility outright (a
/// version published "after" a root would compare as published before
/// it), so publication saturates here instead and every root captured
/// from then on FAILS CLOSED — [`SchedulerSourceRoot::lookup`] answers
/// [`SourceStateAt::Unknown`] for every canonical, so consumers fall back
/// to their own authority rather than reading a world whose ordering can
/// no longer be expressed.
const EXHAUSTED_EPOCH: u64 = u64::MAX;

/// The logical source state a canonical had at one published epoch.
///
/// `Unknown` is not publishable — it is the answer for a canonical that
/// had no published version at all as of the queried epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishedSourceState {
    /// The canonical had no generation-coherent source: it was never
    /// loaded, its generation was bumped past its snapshot, or its node
    /// was removed.
    Absent,
    /// The canonical had a generation-coherent committed source.
    Present { whole_hash: [u8; 16] },
}

/// The as-of answer [`SchedulerSourceRoot::lookup`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStateAt {
    /// The canonical had NO published version at this root's epoch —
    /// the scheduler had never published a source transition for it
    /// that early. Distinct from [`Self::Absent`] (which records a
    /// transition that made the source unavailable), but consumers
    /// asking "is there a source?" treat both as no.
    Unknown,
    /// The canonical was logically source-less at this root's epoch.
    Absent { incarnation: u64, generation: u64 },
    /// The canonical had a coherent committed source at this root's
    /// epoch.
    Present {
        incarnation: u64,
        generation: u64,
        whole_hash: [u8; 16],
    },
}

impl SourceStateAt {
    /// The committed whole-content hash, or `None` when the canonical
    /// had no coherent source at this epoch.
    #[must_use]
    pub fn whole_hash(&self) -> Option<[u8; 16]> {
        match self {
            Self::Present { whole_hash, .. } => Some(*whole_hash),
            Self::Unknown | Self::Absent { .. } => None,
        }
    }

    /// Did this canonical have a coherent committed source?
    #[must_use]
    pub fn is_present(&self) -> bool {
        matches!(self, Self::Present { .. })
    }
}

/// One published version of one canonical's source state.
///
/// A version is born at the epoch its publication reserved and is
/// implicitly retired by its SUCCESSOR's birth: versions in a chain are
/// append-ordered by strictly increasing birth epoch, so the version
/// visible from a root at `epoch` is the last one whose `birth <= epoch`.
/// There is no separate retirement field to keep in sync.
#[derive(Debug, Clone, Copy)]
struct SourceVersion {
    birth: u64,
    incarnation: u64,
    generation: u64,
    state: PublishedSourceState,
}

impl SourceVersion {
    fn as_state_at(&self) -> SourceStateAt {
        match self.state {
            PublishedSourceState::Absent => SourceStateAt::Absent {
                incarnation: self.incarnation,
                generation: self.generation,
            },
            PublishedSourceState::Present { whole_hash } => SourceStateAt::Present {
                incarnation: self.incarnation,
                generation: self.generation,
                whole_hash,
            },
        }
    }
}

/// The publication lock's guarded state: the current epoch plus the
/// registry of every LIVE captured root.
///
/// Both live under ONE mutex so a capture reads the epoch and registers
/// its lease in the same critical section a publication uses to advance
/// the epoch and append its versions. That single lock is what makes
/// publication atomic with the lifecycle transition, and what totally
/// orders a capture against every transition.
#[derive(Debug, Default)]
struct PublishState {
    /// Monotonic membership epoch — the identity of the current root.
    /// Epoch 0 is the empty directory: every publication stamps a birth
    /// of at least 1, so a root at epoch 0 sees nothing, which is
    /// exactly the membership an empty directory has.
    epoch: u64,
    /// `epoch -> number of live roots captured at that epoch`. A
    /// `BTreeMap` so the retention floor is the first key (O(log n)),
    /// never a scan.
    live_roots: BTreeMap<u64, usize>,
}

/// A version append staged by a [`SourcePublication`].
type StagedEntry = (Arc<str>, u64, u64, PublishedSourceState);

/// The versions one lifecycle transition publishes.
///
/// Handed to the [`SchedulerSourceDirectory::publish_transition`]
/// closure, which records the logical source state each affected
/// canonical has AFTER the transition it just performed. Every recorded
/// entry lands in ONE epoch.
#[derive(Debug)]
pub struct SourcePublication {
    entries: SmallVec<[StagedEntry; 2]>,
}

impl SourcePublication {
    fn empty() -> Self {
        Self {
            entries: SmallVec::new(),
        }
    }

    /// Record that `canonical` now has a coherent committed source.
    pub fn present(
        &mut self,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        whole_hash: [u8; 16],
    ) {
        self.entries.push((
            Arc::clone(canonical),
            incarnation,
            generation,
            PublishedSourceState::Present { whole_hash },
        ));
    }

    /// Bump `node`'s generation. This capability exists only on
    /// [`SourcePublication`], which is only handed out while the
    /// publication lock is held, so a generation bump that must be
    /// atomic with publication cannot run outside that hold.
    pub fn bump_node_generation(&self, node: &crate::node::FileNode) -> u64 {
        node.bump_generation(self)
    }

    /// Record that `canonical` now has no coherent committed source —
    /// its generation was bumped, its node was replaced, or its node was
    /// removed.
    pub fn absent(&mut self, canonical: &Arc<str>, incarnation: u64, generation: u64) {
        self.entries.push((
            Arc::clone(canonical),
            incarnation,
            generation,
            PublishedSourceState::Absent,
        ));
    }

    /// Did this transition change nothing observable?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The scheduler-owned MVCC source authority.
///
/// Separate from [`crate::scheduler::Scheduler::nodes`] by design: the
/// node map stays pure execution state, and this directory is the
/// versioned SOURCE-OF-TRUTH history that immutable roots address.
pub struct SchedulerSourceDirectory {
    /// Per-canonical append-ordered version history.
    ///
    /// Chains are short: a sweep trims every version a captured root can
    /// no longer reach, so steady state is one live version plus the
    /// versions genuinely pinned by live roots.
    versions: DashMap<Arc<str>, SmallVec<[SourceVersion; 2]>>,
    /// Epoch + live-root registry, under the ONE publication lock. See
    /// [`PublishState`].
    publish: Mutex<PublishState>,
    /// Canonicals that gained a superseding version since the last
    /// sweep — the sweep's work list, so reclamation is O(recently
    /// edited canonicals) and never O(tracked canonicals).
    ///
    /// A canonical whose predecessors are still pinned by a live root is
    /// re-queued by the sweep, so a pinned chain is revisited (and
    /// drained) once the root drops.
    pending_trim: Mutex<Vec<Arc<str>>>,
    /// Superseding publications since the last sweep; drives the
    /// amortised self-triggered sweep at
    /// [`RECLAIM_TRIGGER_SUPERSESSIONS`].
    supersessions_since_reclaim: AtomicU64,
}

impl std::fmt::Debug for SchedulerSourceDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let publish = self.publish.lock();
        f.debug_struct("SchedulerSourceDirectory")
            .field("epoch", &publish.epoch)
            .field("tracked_canonicals", &self.versions.len())
            .field("live_roots", &publish.live_roots.values().sum::<usize>())
            .finish_non_exhaustive()
    }
}

impl Default for SchedulerSourceDirectory {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerSourceDirectory {
    #[must_use]
    pub fn new() -> Self {
        Self {
            versions: DashMap::new(),
            publish: Mutex::new(PublishState::default()),
            pending_trim: Mutex::new(Vec::new()),
            supersessions_since_reclaim: AtomicU64::new(0),
        }
    }

    /// The current root's epoch.
    #[must_use]
    pub fn current_epoch(&self) -> u64 {
        self.publish.lock().epoch
    }

    /// Capture an immutable, LEASED root of the directory's current
    /// membership.
    ///
    /// O(1): one mutex acquisition, one scalar read, one counter bump —
    /// independent of the number of tracked canonicals or retained
    /// versions. The returned root both names the epoch and keeps every
    /// version visible at that epoch reachable until it drops.
    #[must_use]
    pub fn capture_root(self: &Arc<Self>) -> Arc<SchedulerSourceRoot> {
        let epoch = {
            let mut publish = self.publish.lock();
            let epoch = publish.epoch;
            *publish.live_roots.entry(epoch).or_insert(0) += 1;
            epoch
        };
        Arc::new(SchedulerSourceRoot {
            epoch,
            directory: Arc::clone(self),
        })
    }

    /// Release one lease at `epoch`. Called only from
    /// [`SchedulerSourceRoot::drop`].
    fn release_root(&self, epoch: u64) {
        let mut publish = self.publish.lock();
        if let std::collections::btree_map::Entry::Occupied(mut slot) =
            publish.live_roots.entry(epoch)
        {
            let count = slot.get_mut();
            *count = count.saturating_sub(1);
            if *count == 0 {
                slot.remove();
            }
        }
    }

    /// Run a scheduler lifecycle transition and publish the source
    /// versions it produced, ATOMICALLY.
    ///
    /// `transition` performs the node mutation (bump the generation,
    /// commit the source snapshot, remove the node) and records the
    /// resulting logical state of each affected canonical on the
    /// [`SourcePublication`]. Both halves run under one hold of the
    /// publication lock, and [`Self::capture_root`] takes the same lock,
    /// so no capture can observe a window where the node has
    /// transitioned but the root has not, or the reverse.
    ///
    /// A transition that records nothing publishes nothing and does not
    /// advance the epoch — node creation, for instance, leaves
    /// `try_get_source`'s answer unchanged (a fresh node has no source,
    /// and an untracked canonical already reads
    /// [`SourceStateAt::Unknown`]).
    ///
    /// **Lock rank.** The publication lock is INNER to the DAG lock and
    /// OUTER to the `nodes` / `versions` DashMap shards. `transition`
    /// may take a DashMap shard; it must NEVER take the DAG lock.
    pub fn publish_transition<R>(&self, transition: impl FnOnce(&mut SourcePublication) -> R) -> R {
        let mut publication = SourcePublication::empty();
        let mut superseded = 0u64;
        let mut trimmable: SmallVec<[Arc<str>; 2]> = SmallVec::new();

        let result = {
            let mut publish = self.publish.lock();
            let result = transition(&mut publication);
            if !publication.entries.is_empty() {
                // Saturating, never wrapping — see [`EXHAUSTED_EPOCH`].
                publish.epoch = publish.epoch.checked_add(1).unwrap_or(EXHAUSTED_EPOCH);
                let epoch = publish.epoch;
                for (canonical, incarnation, generation, state) in publication.entries.drain(..) {
                    let mut chain = self.versions.entry(Arc::clone(&canonical)).or_default();
                    let had_predecessor = !chain.is_empty();
                    chain.push(SourceVersion {
                        birth: epoch,
                        incarnation,
                        generation,
                        state,
                    });
                    drop(chain);
                    if had_predecessor {
                        superseded += 1;
                        trimmable.push(canonical);
                    } else if state == PublishedSourceState::Absent {
                        // A first-and-only ABSENT version carries nothing
                        // any root can act on, so the sweep may drop its
                        // whole entry — queue it, but do NOT count it as a
                        // supersession (it is not one, and the sweep
                        // trigger measures real churn).
                        trimmable.push(canonical);
                    }
                }
            }
            result
        };

        if !trimmable.is_empty() {
            self.pending_trim.lock().extend(trimmable);
            let total = self
                .supersessions_since_reclaim
                .fetch_add(superseded, Ordering::Relaxed)
                + superseded;
            if total >= RECLAIM_TRIGGER_SUPERSESSIONS {
                let _ = self.reclaim_superseded_versions();
            }
        }

        result
    }

    /// The epochs a sweep must keep addressable: every live captured
    /// root's epoch, plus the current epoch (which every FUTURE capture
    /// addresses or supersedes). Returned sorted ascending.
    fn retention_epochs(&self) -> SmallVec<[u64; 4]> {
        let publish = self.publish.lock();
        let mut epochs: SmallVec<[u64; 4]> = publish.live_roots.keys().copied().collect();
        if let Err(slot) = epochs.binary_search(&publish.epoch) {
            epochs.insert(slot, publish.epoch);
        }
        epochs
    }

    /// Physically reclaim every superseded version no root can reach.
    ///
    /// **The complete reachability rule:** a version is reclaimable only
    /// when it is invisible from (a) the current root AND (b) every live
    /// captured root. A root at `epoch` selects exactly ONE version per
    /// canonical — the newest one born at or below `epoch` — so the
    /// retained set is the union of those selections over every live root
    /// plus the current one.
    ///
    /// It is deliberately NOT a floor. "Born after the oldest live root"
    /// keeps every successor as well, which turns one stale root into an
    /// unbounded leak: an edit loop under a single pinned view retains a
    /// version per keystroke even though that view can only ever select
    /// the one version its own epoch names.
    ///
    /// The epochs are read under the publication lock and the sweep then
    /// runs without it, exactly as `FileArtifactStore` does: a capture
    /// racing the sweep reads the current epoch or a later one, and the
    /// version selected at the current epoch is retained here, so the
    /// newly captured root can still select its own answer.
    ///
    /// Returns the number of physically reclaimed versions.
    pub fn reclaim_superseded_versions(&self) -> usize {
        let epochs = self.retention_epochs();
        let queued: Vec<Arc<str>> = std::mem::take(&mut *self.pending_trim.lock());
        self.supersessions_since_reclaim.store(0, Ordering::Relaxed);

        let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
        let mut still_retained: Vec<Arc<str>> = Vec::new();
        let mut reclaimed = 0usize;
        for canonical in queued {
            if !seen.insert(Arc::clone(&canonical)) {
                continue;
            }
            let Some(mut chain) = self.versions.get_mut(&canonical) else {
                continue;
            };
            let before = chain.len();
            // Keep exactly the version each retained epoch selects, plus
            // anything born after the newest retained epoch (a version a
            // concurrent publication appended while this sweep ran, which
            // the epochs read above cannot describe).
            let newest_retained_epoch = epochs.last().copied().unwrap_or(0);
            let mut selected: SmallVec<[usize; 4]> = SmallVec::new();
            for &epoch in &epochs {
                if let Some(index) = chain.iter().rposition(|version| version.birth <= epoch) {
                    if selected.last() != Some(&index) {
                        selected.push(index);
                    }
                }
            }
            let mut index = 0usize;
            chain.retain(|version| {
                let keep = version.birth > newest_retained_epoch || selected.contains(&index);
                index += 1;
                keep
            });
            reclaimed += before - chain.len();
            let retained = chain.len();
            // A canonical whose entire retained history is one ABSENT
            // version carries no information any root can act on: every
            // root either predates the version (and already reads
            // `Unknown`) or selects it, and the answer's sole consumer
            // treats `Absent` and `Unknown` identically. Dropping the map
            // entry is therefore observationally inert, and it is what
            // keeps a long-lived process from retaining one
            // `SourceVersion` + `Arc<str>` per canonical it EVER
            // published — closed files included.
            let drop_entry = retained == 1 && chain[0].state == PublishedSourceState::Absent;
            drop(chain);
            if drop_entry {
                // `remove_if` re-checks under the shard guard, so a
                // publication that landed since the `drop` above keeps its
                // entry.
                self.versions.remove_if(&canonical, |_, chain| {
                    chain.len() == 1 && chain[0].state == PublishedSourceState::Absent
                });
            } else if retained > 1 {
                // Still pinned by a live root — revisit it on a later
                // sweep so the retained versions drain once that root
                // drops.
                still_retained.push(canonical);
            }
        }
        if !still_retained.is_empty() {
            self.pending_trim.lock().extend(still_retained);
        }
        reclaimed
    }

    /// Number of live captured roots. Diagnostics and tests.
    #[must_use]
    pub fn live_root_count(&self) -> usize {
        self.publish.lock().live_roots.values().sum()
    }

    /// Seed the publication epoch. Test-only: the epoch line's terminal
    /// behaviour is otherwise unreachable in any finite test.
    #[cfg(any(test, feature = "test-support"))]
    pub fn seed_epoch_for_test(&self, epoch: u64) {
        self.publish.lock().epoch = epoch;
    }

    /// Number of retained versions for `canonical`. Diagnostics and
    /// tests — the production read surface is
    /// [`SchedulerSourceRoot::lookup`].
    #[must_use]
    pub fn retained_version_count(&self, canonical: &str) -> usize {
        self.versions.get(canonical).map_or(0, |chain| chain.len())
    }

    /// THE as-of visibility function: the version of `canonical` visible
    /// from a root at `epoch`.
    ///
    /// One canonical lookup plus a predecessor search in that
    /// canonical's short retained history.
    fn state_at(&self, canonical: &str, epoch: u64) -> SourceStateAt {
        if epoch == EXHAUSTED_EPOCH {
            // The epoch line is exhausted: this root can address no
            // consistent world, so it addresses none at all.
            return SourceStateAt::Unknown;
        }
        let Some(chain) = self.versions.get(canonical) else {
            return SourceStateAt::Unknown;
        };
        chain
            .iter()
            .rev()
            .find(|version| version.birth <= epoch)
            .map_or(SourceStateAt::Unknown, SourceVersion::as_state_at)
    }
}

/// An immutable, LEASED root of [`SchedulerSourceDirectory`] membership.
///
/// Holding one does two inseparable things:
///
/// 1. it NAMES an epoch — every canonical's source state as of that
///    epoch resolves through it, regardless of what the live scheduler
///    nodes now hold; and
/// 2. it KEEPS that state reachable — the directory may not reclaim any
///    version this root can still select.
///
/// [`Self::lookup`] is sealed to the root's epoch: a holder cannot reach
/// the live directory through it, and there is no "read the current
/// state" escape hatch on this type.
///
/// Not `Clone` by design — one value is one registration. Share it by
/// `Arc`; the lease is released once, when the last `Arc` drops.
pub struct SchedulerSourceRoot {
    epoch: u64,
    directory: Arc<SchedulerSourceDirectory>,
}

impl SchedulerSourceRoot {
    /// The epoch this root addresses.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Was this root captured after the epoch line was EXHAUSTED?
    ///
    /// Such a root addresses no published membership: [`Self::lookup`]
    /// answers [`SourceStateAt::Unknown`] for every canonical. See
    /// [`EXHAUSTED_EPOCH`].
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.epoch == EXHAUSTED_EPOCH
    }

    /// The source state `canonical` had AS OF this root's epoch.
    ///
    /// Sealed: the answer never reflects a transition published after
    /// the capture, even when the live node has moved on.
    #[must_use]
    pub fn lookup(&self, canonical: &str) -> SourceStateAt {
        self.directory.state_at(canonical, self.epoch)
    }
}

impl std::fmt::Debug for SchedulerSourceRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchedulerSourceRoot")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl Drop for SchedulerSourceRoot {
    fn drop(&mut self) {
        self.directory.release_root(self.epoch);
    }
}
