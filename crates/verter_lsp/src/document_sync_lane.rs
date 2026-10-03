//! The ONE per-document provider-sync lane, and its generation-keyed registry.
//!
//! Every transaction that delivers or commits an open document's provider
//! surface — the interactive request repair, the debounced coordinator, the
//! synchronous and detached API syncs, the background carrier resync, the
//! snapshot/pending drains and the workspace scanner — serializes on the lane
//! this registry hands out for `(canonical id, open generation)`. Without one
//! shared owner, two transactions of one revision interleave: the later commit
//! is refused at an equal key, or overwrites the earlier one's whole state, and
//! a request captured between them fails closed to a reduced answer.
//!
//! # Lock order
//!
//! Every path takes these in exactly this order, and no path takes them twice:
//!
//! 1. the per-document sync lane (including lifecycle open/close),
//! 2. the lifecycle/global-commit mutex (`did_change_mutex`), when needed,
//! 3. the per-path provider delivery lock,
//! 4. the provider-hub single-writer actor.
//!
//! An open generation is minted only under the document's lifecycle lane, so a
//! writer probing mid-open yields to the open instead of minting its own.
//!
//! Edit commits take only the global-commit mutex; they never await a document
//! lane while holding it. Background writers try the lane and requeue busy
//! documents; imported-carrier sync, which already waits for the lane before
//! its IDE leg, waits for it again for its API leg. API-only delivery releases the document lane for provider I/O,
//! then tries it again and validates the exact basis and its own delivery before
//! admitting only the API fields. No path reacquires a lane it already holds.

use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// One generation-aware per-document sync lane.
///
/// Retirement belongs to the lane object, never merely to its canonical-id key,
/// so a stale close cannot retire a reopened document's replacement lane (the
/// key-reuse/ABA case).
///
/// The mutex is held behind an `Arc` so a lease can hand out an *owned* guard:
/// a background caller that asks `try_lock` and loses the race drops the lease
/// immediately, while a winner keeps its guard across awaits without borrowing
/// the registry.
pub(crate) struct DocumentSyncLane {
    mutex: Arc<tokio::sync::Mutex<()>>,
    generation: AtomicU64,
    /// Advances once an admitted projection-less repair has attempted its
    /// compile. Waiters that observed the prior value join that attempt; a
    /// later request observes the new value and may retry transient failure.
    repair_sequence: AtomicU64,
    retired: AtomicBool,
}

impl DocumentSyncLane {
    fn new(generation: u64) -> Self {
        Self {
            mutex: Arc::new(tokio::sync::Mutex::new(())),
            generation: AtomicU64::new(generation),
            repair_sequence: AtomicU64::new(0),
            retired: AtomicBool::new(false),
        }
    }

    /// Whether a close has retired this lane object. A stale prior-generation
    /// repair must not retire the reopened document's replacement lane.
    pub(crate) fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }

    fn serves_generation(&self, generation: u64) -> bool {
        !self.is_retired() && self.generation.load(Ordering::Acquire) == generation
    }
}

/// The shared owner of every document's sync lane.
///
/// This is the single place that maps `(canonical id, open generation)` onto a
/// lane and retires it. It is reachable from every writer — the server, the
/// sync coordinator, the background drains and the workspace scanner all hold
/// the same `Arc` — so there is exactly one lane per open document.
pub(crate) struct DocumentSyncLanes {
    lanes: DashMap<String, Arc<DocumentSyncLane>>,
    /// Current open-document generation per canonical id. `did_open` mints one,
    /// `did_close` removes only its exact generation, so a reopen can never be
    /// mistaken for the document instance that initiated stale work.
    open_generations: DashMap<String, u64>,
    next_generation: AtomicU64,
}

impl Default for DocumentSyncLanes {
    fn default() -> Self {
        Self {
            lanes: DashMap::new(),
            open_generations: DashMap::new(),
            next_generation: AtomicU64::new(1),
        }
    }
}

/// One participant in a document's generation-bound sync lane. A closed lane is
/// retired synchronously by the final participant's drop, so cleanup is
/// event-driven and never needs a polling task.
pub(crate) struct DocumentLaneLease {
    canonical_id: String,
    lane: Arc<DocumentSyncLane>,
    lanes: Arc<DocumentSyncLanes>,
}

impl DocumentLaneLease {
    /// The lease's lane object. Identity (not the canonical-id key) is what
    /// retirement and removal are keyed on.
    pub(crate) fn lane(&self) -> &Arc<DocumentSyncLane> {
        &self.lane
    }

    /// Wait for the lane. Interactive paths only: the freshness re-check under
    /// the lane turns the wait into a coalesced no-op rather than a second
    /// transaction.
    pub(crate) async fn lock(&self) -> tokio::sync::OwnedMutexGuard<()> {
        Arc::clone(&self.lane.mutex).lock_owned().await
    }

    /// Take the lane only if it is free right now. Background paths ask this
    /// way so a busy document YIELDS the transaction instead of stalling a
    /// serial loop or queueing ahead of an interactive request.
    pub(crate) fn try_lock(&self) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        Arc::clone(&self.lane.mutex).try_lock_owned().ok()
    }

    pub(crate) fn retire(&self) {
        self.lane.retired.store(true, Ordering::Release);
    }

    pub(crate) fn generation(&self) -> u64 {
        self.lane.generation.load(Ordering::Acquire)
    }

    pub(crate) fn repair_sequence(&self) -> u64 {
        self.lane.repair_sequence.load(Ordering::Acquire)
    }

    pub(crate) fn complete_repair_attempt(&self) {
        self.lane.repair_sequence.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for DocumentLaneLease {
    fn drop(&mut self) {
        if !self.lane.is_retired() {
            return;
        }
        self.lanes
            .lanes
            .remove_if(&self.canonical_id, |_, current| {
                Arc::ptr_eq(current, &self.lane) && Arc::strong_count(&self.lane) == 2
            });
    }
}

/// The outcome of asking for a document's delivery lane at the moment of
/// delivery — the point where a transaction that started while its document was
/// closed can discover that it is now open.
pub(crate) enum DeliveryLane {
    /// The document is closed: there is no open generation, so there is no
    /// interactive repair to interleave with and the transaction proceeds.
    Closed,
    /// The lane was free and is now held by the returned guard.
    Acquired(tokio::sync::OwnedMutexGuard<()>),
    /// Another transaction holds this document's lane. A background caller
    /// YIELDS: it re-arms the document and delivers nothing this pass.
    Busy,
}

/// A document's open generation, as established by a writer that may run before
/// `did_open` finished minting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EstablishedGeneration {
    /// The document is open under this generation.
    Open(u64),
    /// The document is not registered: no editor has it open.
    Closed,
    /// The document's lifecycle lane is held (an open or close is mid-flight),
    /// so its generation cannot be established without waiting.
    Busy,
}

/// How a transaction asks for a document's delivery lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneAcquire {
    /// Take the lane only if it is free right now; a busy document yields and
    /// is requeued. Background writers and the coordinator's serial loop.
    Try,
    /// Wait for the lane. Only for a writer that is already ordered on the
    /// document's lifecycle lane (imported-carrier sync): it released that
    /// lane between its legs, and a waiter queued behind it would otherwise
    /// take its turn and leave its API leg requeued with nothing to redrive it.
    Wait,
}

impl std::fmt::Debug for DeliveryLane {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => formatter.write_str("Closed"),
            Self::Acquired(_) => formatter.write_str("Acquired"),
            Self::Busy => formatter.write_str("Busy"),
        }
    }
}

impl DocumentSyncLanes {
    /// Acquire the current lifecycle lane. Open and close use this before
    /// mutating registry membership, so a reopen cannot land in the middle of a
    /// close of the prior document generation.
    pub(crate) fn lifecycle_lease(self: &Arc<Self>, canonical_id: &str) -> DocumentLaneLease {
        let lane = match self.lanes.entry(canonical_id.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                if entry.get().is_retired() {
                    let generation = self.open_generation(canonical_id).unwrap_or(0);
                    let replacement = Arc::new(DocumentSyncLane::new(generation));
                    entry.insert(Arc::clone(&replacement));
                    replacement
                } else {
                    Arc::clone(entry.get())
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let generation = self.open_generation(canonical_id).unwrap_or(0);
                let lane = Arc::new(DocumentSyncLane::new(generation));
                entry.insert(Arc::clone(&lane));
                lane
            }
        };
        DocumentLaneLease {
            canonical_id: canonical_id.to_string(),
            lane,
            lanes: Arc::clone(self),
        }
    }

    /// Acquire only the lane belonging to `generation`. A stale transaction
    /// never inserts or replaces the lane of a closed/reopened document: it
    /// receives a detached retired lane, fails generation revalidation after
    /// locking, and disappears on drop without touching the map.
    pub(crate) fn repair_lease(
        self: &Arc<Self>,
        canonical_id: &str,
        generation: u64,
    ) -> DocumentLaneLease {
        let detached = || {
            let lane = Arc::new(DocumentSyncLane::new(generation));
            lane.retired.store(true, Ordering::Release);
            lane
        };
        let generation_is_current = self.open_generation(canonical_id) == Some(generation);
        let lane = if generation_is_current {
            match self.lanes.entry(canonical_id.to_string()) {
                dashmap::mapref::entry::Entry::Occupied(entry)
                    if entry.get().serves_generation(generation) =>
                {
                    Arc::clone(entry.get())
                }
                dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                    // Re-check while owning the map entry. If this caller lost
                    // the generation race, it must not replace the winner's lane.
                    if self.open_generation(canonical_id) == Some(generation) {
                        let replacement = Arc::new(DocumentSyncLane::new(generation));
                        entry.insert(Arc::clone(&replacement));
                        replacement
                    } else {
                        detached()
                    }
                }
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    if self.open_generation(canonical_id) == Some(generation) {
                        let lane = Arc::new(DocumentSyncLane::new(generation));
                        entry.insert(Arc::clone(&lane));
                        lane
                    } else {
                        detached()
                    }
                }
            }
        } else {
            detached()
        };
        DocumentLaneLease {
            canonical_id: canonical_id.to_string(),
            lane,
            lanes: Arc::clone(self),
        }
    }

    /// Take the lane of a document's CURRENT open generation, without waiting
    /// and without ever creating or ROTATING a generation.
    ///
    /// This is the background-writer entry point. `Closed` is the answer for a
    /// document no editor has open — there is no interactive transaction to
    /// interleave with — and `Busy` is the answer when an interactive repair
    /// already owns the document. Neither rotates a generation, so a caller that
    /// loses the race leaves the map exactly as it found it.
    pub(crate) fn try_delivery_lane(self: &Arc<Self>, canonical_id: &str) -> DeliveryLane {
        let Some(generation) = self.open_generation(canonical_id) else {
            return DeliveryLane::Closed;
        };
        let lease = self.repair_lease(canonical_id, generation);
        let Some(guard) = lease.try_lock() else {
            return DeliveryLane::Busy;
        };
        if !lease.lane.serves_generation(generation)
            || self.open_generation(canonical_id) != Some(generation)
        {
            return DeliveryLane::Busy;
        }
        DeliveryLane::Acquired(guard)
    }

    /// [`Self::try_delivery_lane`] that waits for a busy lane instead of
    /// yielding. A close or reopen while waiting answers `Busy`: the waiter's
    /// generation is gone and it commits nothing into the replacement.
    pub(crate) async fn wait_delivery_lane(self: &Arc<Self>, canonical_id: &str) -> DeliveryLane {
        let Some(generation) = self.open_generation(canonical_id) else {
            return DeliveryLane::Closed;
        };
        let lease = self.repair_lease(canonical_id, generation);
        let guard = lease.lock().await;
        if !lease.lane.serves_generation(generation)
            || self.open_generation(canonical_id) != Some(generation)
        {
            return DeliveryLane::Busy;
        }
        DeliveryLane::Acquired(guard)
    }

    pub(crate) fn open_generation(&self, canonical_id: &str) -> Option<u64> {
        self.open_generations.get(canonical_id).map(|entry| *entry)
    }

    /// Establish the open generation of a registered document that has none,
    /// without waiting: [`EstablishedGeneration::Busy`] when its lifecycle lane
    /// is held.
    ///
    /// A generation is minted ONLY under the document's lifecycle lane, exactly
    /// as `did_open` mints one. `did_open` registers the document before it
    /// mints the generation, all under that lane, so a writer probing in that
    /// window finds the lane held and yields; minting outside it would hand the
    /// writer a second lane object for the same open document. `is_registered`
    /// is re-read under the lane, so a close that won the race is never
    /// resurrected.
    pub(crate) fn try_establish_open_generation(
        self: &Arc<Self>,
        canonical_id: &str,
        is_registered: impl Fn() -> bool,
    ) -> EstablishedGeneration {
        if let Some(generation) = self.open_generation(canonical_id) {
            return EstablishedGeneration::Open(generation);
        }
        if !is_registered() {
            return EstablishedGeneration::Closed;
        }
        let lease = self.lifecycle_lease(canonical_id);
        let Some(_guard) = lease.try_lock() else {
            return EstablishedGeneration::Busy;
        };
        self.establish_under_lifecycle(canonical_id, &lease, is_registered)
    }

    /// [`Self::try_establish_open_generation`] that waits for a held lifecycle
    /// lane instead of yielding. `None` for a document that is not registered.
    pub(crate) async fn wait_establish_open_generation(
        self: &Arc<Self>,
        canonical_id: &str,
        is_registered: impl Fn() -> bool,
    ) -> Option<u64> {
        if let Some(generation) = self.open_generation(canonical_id) {
            return Some(generation);
        }
        if !is_registered() {
            return None;
        }
        let lease = self.lifecycle_lease(canonical_id);
        let _guard = lease.lock().await;
        match self.establish_under_lifecycle(canonical_id, &lease, is_registered) {
            EstablishedGeneration::Open(generation) => Some(generation),
            EstablishedGeneration::Closed | EstablishedGeneration::Busy => None,
        }
    }

    /// Mint the generation while the caller holds `lease`'s lifecycle lane.
    fn establish_under_lifecycle(
        &self,
        canonical_id: &str,
        lease: &DocumentLaneLease,
        is_registered: impl Fn() -> bool,
    ) -> EstablishedGeneration {
        if let Some(generation) = self.open_generation(canonical_id) {
            return EstablishedGeneration::Open(generation);
        }
        if !is_registered() {
            // The lane this probe installed serves no open document: retire it
            // so the final lease drop removes it. A concurrent open revives it
            // in place.
            lease.retire();
            return EstablishedGeneration::Closed;
        }
        EstablishedGeneration::Open(self.begin_open_generation(canonical_id, lease.lane()))
    }

    pub(crate) fn begin_open_generation(
        &self,
        canonical_id: &str,
        lane: &Arc<DocumentSyncLane>,
    ) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        lane.generation.store(generation, Ordering::Release);
        lane.repair_sequence.store(0, Ordering::Release);
        lane.retired.store(false, Ordering::Release);
        self.lanes
            .insert(canonical_id.to_string(), Arc::clone(lane));
        self.open_generations
            .insert(canonical_id.to_string(), generation);
        generation
    }

    pub(crate) fn generation_is_open(&self, canonical_id: &str, generation: u64) -> bool {
        self.open_generation(canonical_id) == Some(generation)
    }

    /// Remove ONLY the named generation. A close of a prior incarnation can
    /// therefore never retire the generation a reopen just minted.
    pub(crate) fn close_open_generation(&self, canonical_id: &str, generation: u64) {
        self.open_generations
            .remove_if(canonical_id, |_, current| *current == generation);
    }
}

/// Test-only observation surface. The lifecycle invariants these expose —
/// which lane object is mapped, whose generation it claims, whether a close
/// retired it — are observable behaviour, not production control flow, so they
/// stay out of the shipped API.
#[cfg(test)]
impl DocumentSyncLane {
    /// The open generation this lane serves. A mapped lane's generation is its
    /// identity claim: after a reopen the mapped lane must belong to the NEW
    /// generation, never the prior incarnation's.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

#[cfg(test)]
impl DocumentLaneLease {
    pub(crate) fn is_retired(&self) -> bool {
        self.lane.is_retired()
    }
}

#[cfg(test)]
impl DocumentSyncLanes {
    /// Whether a lane object is currently installed for this canonical id. The
    /// lane map, not the generation map: retirement removes the LANE object, so
    /// this is the observable a close/reopen lifecycle test asserts on.
    pub(crate) fn has_lane(&self, canonical_id: &str) -> bool {
        self.lanes.contains_key(canonical_id)
    }

    /// The lane object currently mapped to this canonical id, if any. The
    /// identity of the returned `Arc` is what proves a close did not split one
    /// document's lane into two mutexes.
    pub(crate) fn lane(&self, canonical_id: &str) -> Option<Arc<DocumentSyncLane>> {
        self.lanes
            .get(canonical_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// The canonical ids that currently own a lane. Diagnostic for a failing
    /// "no lanes remain" assertion.
    pub(crate) fn mapped_canonical_ids(&self) -> Vec<String> {
        self.lanes.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Whether the document currently has an open generation. A close removes
    /// only its exact generation, so this stays true across an overlapping
    /// reopen and false only once no editor holds the document open.
    pub(crate) fn has_open_generation(&self, canonical_id: &str) -> bool {
        self.open_generations.contains_key(canonical_id)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    pub(crate) fn has_no_open_generations(&self) -> bool {
        self.open_generations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lanes() -> Arc<DocumentSyncLanes> {
        Arc::new(DocumentSyncLanes::default())
    }

    #[test]
    fn a_closed_document_has_no_delivery_lane() {
        let lanes = lanes();
        assert!(matches!(
            lanes.try_delivery_lane("/workspace/src/App.vue"),
            DeliveryLane::Closed
        ));
    }

    #[test]
    fn the_first_delivery_probe_installs_the_open_generations_lane() {
        let lanes = lanes();
        let canonical = "/workspace/src/App.vue";
        let EstablishedGeneration::Open(generation) =
            lanes.try_establish_open_generation(canonical, || true)
        else {
            panic!("a registered document with a free lifecycle lane establishes its generation");
        };
        let DeliveryLane::Acquired(guard) = lanes.try_delivery_lane(canonical) else {
            panic!("an open generation without a lane must acquire its new lane");
        };
        let repair = lanes.repair_lease(canonical, generation);
        assert!(
            repair.try_lock().is_none(),
            "repair shares the lane just installed by delivery"
        );
        drop(guard);
        assert!(repair.try_lock().is_some());
    }

    #[tokio::test]
    async fn a_writer_probing_mid_open_yields_instead_of_minting_a_second_lane() {
        let lanes = lanes();
        let canonical = "/workspace/src/App.vue";
        // `did_open` holds the lifecycle lane and has registered the document,
        // but has not minted its generation yet.
        let open = lanes.lifecycle_lease(canonical);
        let opening = open.lock().await;
        assert_eq!(
            lanes.try_establish_open_generation(canonical, || true),
            EstablishedGeneration::Busy,
            "a writer in the open window yields to the open"
        );
        assert!(lanes.open_generation(canonical).is_none());
        let generation = lanes.begin_open_generation(canonical, open.lane());
        drop(opening);

        let DeliveryLane::Acquired(guard) = lanes.try_delivery_lane(canonical) else {
            panic!("the open's own lane is free once the open finishes");
        };
        let repair = lanes.repair_lease(canonical, generation);
        assert!(
            Arc::ptr_eq(repair.lane(), open.lane()),
            "every writer of the generation shares the lane the open minted"
        );
        assert!(repair.try_lock().is_none(), "the delivery holds that lane");
        drop(guard);
    }

    #[tokio::test]
    async fn an_open_document_hands_its_lane_to_one_writer_at_a_time() {
        let lanes = lanes();
        let canonical = "/workspace/src/App.vue";
        let lease = lanes.lifecycle_lease(canonical);
        let guard = lease.lock().await;
        lanes.begin_open_generation(canonical, lease.lane());

        let contenders = Arc::new(lanes.clone());
        let busy = {
            let contenders = Arc::clone(&contenders);
            let canonical = canonical.to_string();
            tokio::spawn(async move { contenders.try_delivery_lane(&canonical) })
        };
        assert!(matches!(
            busy.await.expect("the contender finishes"),
            DeliveryLane::Busy
        ));

        drop(guard);
        match contenders.try_delivery_lane(canonical) {
            DeliveryLane::Acquired(guard) => drop(guard),
            other => panic!("the released lane must be acquirable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_waiting_writer_takes_its_turn_but_never_a_closed_generations_lane() {
        let lanes = lanes();
        let canonical = "/workspace/src/App.vue";
        let generation = lanes
            .wait_establish_open_generation(canonical, || true)
            .await
            .expect("a registered document establishes its generation");

        // A waiter queued behind the holder gets the lane on release instead
        // of yielding it, which a try would do.
        let holder = lanes.repair_lease(canonical, generation);
        let held = holder.lock().await;
        let waiter = tokio::spawn({
            let lanes = Arc::clone(&lanes);
            async move { lanes.wait_delivery_lane(canonical).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "the waiter waits for the holder");
        drop(held);
        let DeliveryLane::Acquired(guard) = waiter.await.expect("waiter task") else {
            panic!("the waiter takes the released lane of its own generation");
        };
        drop(guard);

        // A waiter whose generation closes while it waits takes nothing.
        let held = holder.lock().await;
        let waiter = tokio::spawn({
            let lanes = Arc::clone(&lanes);
            async move { lanes.wait_delivery_lane(canonical).await }
        });
        tokio::task::yield_now().await;
        lanes.close_open_generation(canonical, generation);
        drop(held);
        assert!(
            matches!(waiter.await.expect("waiter task"), DeliveryLane::Busy),
            "a waiter of a closed generation yields instead of delivering"
        );
    }

    #[tokio::test]
    async fn a_stale_generation_never_takes_the_reopened_document_lane() {
        let lanes = lanes();
        let canonical = "/workspace/src/App.vue";
        let stale = lanes.repair_lease(canonical, 7);
        assert!(
            stale.lane().is_retired(),
            "a lane for an unknown generation is detached"
        );
        let stale_guard = stale.lock().await;
        drop(stale_guard);

        let live = lanes.lifecycle_lease(canonical);
        let live_guard = live.lock().await;
        let reopened = lanes.begin_open_generation(canonical, live.lane());
        assert_ne!(reopened, 7);
        assert!(
            matches!(lanes.try_delivery_lane(canonical), DeliveryLane::Busy),
            "the live lane is held"
        );
        drop(live_guard);
    }

    #[test]
    fn a_close_removes_only_its_own_generation() {
        let lanes = lanes();
        let canonical = "/workspace/src/App.vue";
        let lease = lanes.lifecycle_lease(canonical);
        let first = lanes.begin_open_generation(canonical, lease.lane());
        let second = lanes.begin_open_generation(canonical, lease.lane());
        lanes.close_open_generation(canonical, first);
        assert_eq!(
            lanes.open_generation(canonical),
            Some(second),
            "a close of the prior incarnation must not retire the reopened generation"
        );
    }
}
