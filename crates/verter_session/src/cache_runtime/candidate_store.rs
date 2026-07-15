//! Reverse-indexed multi-candidate query-identity store.
//!
//! The query-identity caches whose slot key is content-free but which
//! also need a per-canonical reverse index for O(K) invalidation
//! (imported-registry, materialise-structure, ref-cycle) share this one
//! store. It owns:
//!
//!   - a bounded multi-candidate slot map (cap
//!     [`crate::cache_runtime::node::QUERY_SLOT_CANDIDATE_CAP`]). Two
//!     candidates in one slot belong to different views (a base view and
//!     an overlay view, or two overlays); a re-publish under the SAME
//!     view replaces in place, a different view admits a distinct
//!     candidate, and a fresh candidate past the cap FIFO-evicts the
//!     oldest. Replacement / admission identity is the
//!     [`FactCandidateDiscriminant`] (generation + facts) — NOT the
//!     read-side validity oracle. Preserves R20 overlay isolation: a base
//!     publish never clobbers a live overlay candidate on the same
//!     content-free key.
//!   - a per-canonical reverse index keyed `canonical -> {(key,
//!     admission_seq)}`, so a per-canonical invalidation drains in O(K)
//!     and a removal is identity-scoped to the exact admission
//!     (`admission_seq`) — a concurrent same-key re-publish carrying a
//!     distinct seq survives.
//!   - an optional FIFO [`GlobalRetentionBudget`] (the budgeted consumers
//!     pass a cap; the imported-registry consumer passes `None` and
//!     relies on the per-slot cap plus per-canonical drain).
//!   - a shared `live_counter` and a `retention_gate` (the publish
//!     fence).
//!
//! ## Split publish lifecycle
//!
//! [`Self::publish_core`] runs the non-reentrant publish step under the
//! slot's write guard: install/replace the candidate, net-bump the live
//! counter, register the reverse index under every canonical the
//! candidate's facts name, and record the retention admission — capturing
//! any FIFO victims. [`Self::evict_deferred`] then removes those victims
//! AFTER the slot guard has dropped (still inside the caller's retention
//! read guard), so a budgeted eviction — which re-enters the slot map and
//! reverse index — never self-deadlocks on the publish-core guard.
//!
//! A stale candidate is skipped on read (the slot keeps it for other
//! views) — read never reclaims. Every actual removal path (per-slot FIFO
//! eviction, deferred budget victim, per-canonical drain, schema eviction,
//! project-generation `clear`) runs the SAME identity-scoped cleanup
//! EXACTLY ONCE: drop the candidate, decrement the counter, drain its
//! reverse-index registrations, and forget its retention-ledger record.

use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use rustc_hash::{FxHashMap, FxHasher};

use super::admission::{
    Candidate, DeferredVictims, FactCandidateDiscriminant, PublishCoreOutcome, PublishOutcome,
};
use super::node::QUERY_SLOT_CANDIDATE_CAP;
use crate::bounded_query_retention::GlobalRetentionBudget;
// Used only by the test/debug-gated `insert_for_test` surface below; gated
// to match so release builds do not flag it as unused.
#[cfg(any(test, feature = "test-support"))]
use crate::fact_signature_helpers::ReadSetSignature;

/// A candidate stored in the multi-candidate store. The carried
/// [`Candidate`] discriminant is the [`FactCandidateDiscriminant`]; its
/// `admission_seq` is the store-assigned per-publish identity.
type StoredCandidate<V> = Arc<Candidate<FactCandidateDiscriminant, V>>;

/// Per-canonical reverse index: `canonical -> {(key, admission_seq)}`. The
/// value is a set so a key with several admitted seqs (distinct
/// candidates) registers each under the canonical, and a drain returns
/// every `(key, seq)` for O(K) per-canonical invalidation.
type CanonicalReverseIndexMap<K> =
    DashMap<Arc<str>, Mutex<FxHashMap<(K, u64), ()>>, std::hash::BuildHasherDefault<FxHasher>>;

/// One slot: a bounded, insertion-ordered candidate list behind a write
/// lock, held in the outer map behind an `Arc` so a reader can clone the
/// slot handle and validate candidates without holding the outer shard
/// guard.
struct CandidateSlot<V> {
    candidates: RwLock<Vec<StoredCandidate<V>>>,
}

impl<V> CandidateSlot<V> {
    fn new() -> Self {
        Self {
            candidates: RwLock::new(Vec::new()),
        }
    }
}

/// Shared reverse-indexed multi-candidate query-identity store.
///
/// `allow(dead_code)`: the query-identity cache families
/// (imported-registry, materialise-structure, ref-cycle) route through
/// this; the API is exercised by the `cache_runtime` tests independent of
/// any particular consumer.
#[allow(dead_code)]
pub(crate) struct ReverseIndexedCandidateStore<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Multi-candidate slots, keyed by the content-free slot key.
    slots: DashMap<K, Arc<CandidateSlot<V>>, std::hash::BuildHasherDefault<FxHasher>>,
    /// Per-canonical reverse index — see [`CanonicalReverseIndexMap`].
    canonical_index: CanonicalReverseIndexMap<K>,
    /// Optional FIFO total-size budget. `Some` for the budgeted consumers
    /// (materialise-structure / ref-cycle); `None` for imported-registry,
    /// which relies on the per-slot cap and per-canonical drain.
    retention_budget: Option<GlobalRetentionBudget<(K, u64)>>,
    /// Shared live-candidate counter (the `component_meta_cache_live`
    /// sum). Net-bumped on admission, decremented on every removal.
    live_counter: Arc<AtomicU64>,
    /// Lifecycle gate.
    ///
    /// `publish_core` and `evict_deferred` (the publish side) run under
    /// the shared `read()` guard so unrelated publishes proceed
    /// concurrently. The `invalidate_canonical` per-canonical drain
    /// takes the `write()` guard so it is exclusive against every
    /// in-flight publish — this is what makes a publisher's
    /// reverse-index registration *visible* to a drain that started
    /// after the publisher began but races its `candidates.push` /
    /// `canonical_index.insert` interval (see
    /// [`Self::invalidate_canonical`] for the race rationale). The
    /// project-generation `clear` / schema eviction also takes
    /// `write()`. The query-lookup adapter holds the read guard across
    /// post-compute revalidation, the whole publish-core, and the
    /// deferred eviction, so neither a drain nor a clear can
    /// interleave.
    retention_gate: RwLock<()>,
    /// Monotonic admission-sequence allocator for candidates in this
    /// store. Each published candidate carries a unique `admission_seq`
    /// that doubles as its FIFO order and its reverse-index / budget
    /// removal identity.
    next_seq: AtomicU64,
    /// Per-slot candidate cap.
    per_slot_cap: usize,
    /// Per-store test-only injection point inside [`Self::publish_core`],
    /// fired AFTER the candidate has been pushed into the slot's
    /// candidate vector and BEFORE the per-canonical reverse-index
    /// insert loop runs. With the slot write guard and the retention
    /// gate read guard BOTH still held. A race test arms it and, with
    /// the publisher parked here, runs `invalidate_canonical` on a
    /// canonical the pushed candidate references — the
    /// `retention_gate.write()` acquire in `invalidate_canonical` must
    /// block on the parked publisher's read guard; when the publisher
    /// is released, the invalidator sees the freshly registered entry
    /// and drains it.
    ///
    /// **Per-store scope (test hermeticity).** Per-store, never a
    /// process-global. `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    publish_post_push_pre_register_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[allow(dead_code)]
impl<K, V> ReverseIndexedCandidateStore<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Construct a store with no retention budget (per-slot cap + the
    /// per-canonical drain are the only reclamation). The
    /// imported-registry consumer uses this form.
    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self::build(live_counter, None, QUERY_SLOT_CANDIDATE_CAP)
    }

    /// Construct a store with a FIFO retention budget capping the total
    /// candidate count across all slots. The materialise-structure and
    /// ref-cycle consumers use this form — the budget is their routine
    /// reclamation path.
    pub(crate) fn with_counter_and_budget(live_counter: Arc<AtomicU64>, budget_cap: usize) -> Self {
        Self::build(
            live_counter,
            Some(GlobalRetentionBudget::new(budget_cap)),
            QUERY_SLOT_CANDIDATE_CAP,
        )
    }

    fn build(
        live_counter: Arc<AtomicU64>,
        retention_budget: Option<GlobalRetentionBudget<(K, u64)>>,
        per_slot_cap: usize,
    ) -> Self {
        Self {
            slots: DashMap::with_hasher(std::hash::BuildHasherDefault::<FxHasher>::default()),
            canonical_index: DashMap::with_hasher(
                std::hash::BuildHasherDefault::<FxHasher>::default(),
            ),
            retention_budget,
            live_counter,
            retention_gate: RwLock::new(()),
            next_seq: AtomicU64::new(1),
            per_slot_cap: per_slot_cap.max(1),
            #[cfg(any(test, feature = "test-support"))]
            publish_post_push_pre_register_gate: parking_lot::Mutex::new(None),
        }
    }

    /// Arm the `publish_post_push_pre_register_gate` test injection
    /// point on this store. Once armed, the NEXT [`Self::publish_core`]
    /// call will park between `candidates.push(new_candidate)` and the
    /// per-canonical reverse-index insert loop, holding the slot write
    /// guard and the `retention_gate.read()` guard. `Some(barrier)`
    /// arms; `None` disarms. The barrier must be configured for two
    /// participants (the publisher + the test thread that races
    /// `invalidate_canonical`). The test calls `barrier.wait()` twice:
    /// once to release the publisher into the parked state, once to
    /// release it from the parked state.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn test_arm_publish_post_push_pre_register_gate(
        &self,
        barrier: Option<Arc<std::sync::Barrier>>,
    ) {
        *self.publish_post_push_pre_register_gate.lock() = barrier;
    }

    /// The lifecycle retention gate — the `publish_fence` the query-lookup
    /// adapter holds across revalidation + publish-core + deferred
    /// eviction.
    pub(crate) fn retention_gate(&self) -> &RwLock<()> {
        &self.retention_gate
    }

    /// The configured total retention cap (the budgeted form), or the
    /// per-slot cap when there is no global budget.
    pub(crate) fn retention_cap(&self) -> usize {
        match &self.retention_budget {
            Some(b) => b.cap(),
            None => self.per_slot_cap,
        }
    }

    /// Allocate the next per-publish admission sequence for this store.
    fn alloc_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Total live candidate count across every slot — the store's
    /// authoritative occupancy.
    pub(crate) fn live_count(&self) -> usize {
        self.slots
            .iter()
            .map(|slot| slot.value().candidates.read().len())
            .sum()
    }

    /// Read the slot for the first candidate that passes `accept`.
    ///
    /// The candidate `Arc` is cloned out before `accept` runs, so a
    /// concurrent removal never invalidates the borrow. `accept` is the
    /// caller's read-side validator (generation gate +
    /// `signature.validate_with_self_roots` + fact-bubble side effect);
    /// the discriminant is NOT consulted here — it is admission identity,
    /// not the read oracle. Returns the first accepted candidate's value.
    pub(crate) fn lookup<F>(&self, key: &K, mut accept: F) -> Option<V>
    where
        F: FnMut(&Candidate<FactCandidateDiscriminant, V>) -> Option<V>,
    {
        let snapshot = {
            let slot = self.slots.get(key)?;
            let guard = slot.value().candidates.read();
            guard.clone()
        };
        for candidate in &snapshot {
            if let Some(value) = accept(candidate) {
                return Some(value);
            }
        }
        None
    }

    /// Non-reentrant publish step — runs under the slot write guard.
    ///
    /// Installs/replaces the candidate by its [`FactCandidateDiscriminant`]
    /// (same generation + facts replaces in place and re-enters the FIFO
    /// order as newest; a different view appends a distinct candidate; a
    /// fresh candidate past the per-slot cap FIFO-evicts the oldest),
    /// net-bumps the live counter, registers the reverse index under every
    /// canonical the candidate's facts name, and records the retention
    /// admission. FIFO retention victims are returned for deferred
    /// eviction — NOT evicted here, because eviction re-enters the slot
    /// map and would self-deadlock on this guard.
    ///
    /// The caller (`query::lookup` via the lookup-publish adapter) holds
    /// the [`Self::retention_gate`] read guard across this call AND the
    /// subsequent [`Self::evict_deferred`], so a `clear` cannot interleave.
    pub(crate) fn publish_core(
        &self,
        key: K,
        mut candidate: Candidate<FactCandidateDiscriminant, V>,
    ) -> PublishCoreOutcome<K> {
        // Stamp the store-assigned admission identity.
        let seq = self.alloc_seq();
        candidate.admission_seq = seq;
        // The reverse index covers every canonical a per-canonical
        // invalidation must reach: the facts' canonicals AND the strict
        // self-root canonicals (a candidate with no facts still self-roots
        // its keyed canonical, e.g. the imported-registry keyed canonical).
        let canonicals = reverse_index_canonicals(&candidate);

        // Net-counter accounting: an in-place replace leaves occupancy
        // unchanged; a fresh append is +1; each per-slot FIFO victim is
        // -1.
        let mut fresh = false;
        let mut per_slot_evicted: Vec<(K, u64)> = Vec::new();

        // Hold the slot's `DashMap` shard guard (the `RefMut` from
        // `entry().or_insert_with()`) AND the slot's candidate write guard
        // across the WHOLE non-reentrant publish step — the candidate
        // install, the live-counter bump, the reverse-index registration,
        // AND the retention `record_admission`. A concurrent
        // `remove_candidate_by_seq` (called from `evict_deferred` / a
        // post-drain seq removal) takes the SAME slot write guard before
        // it can observe / remove this candidate, so it can never observe
        // a published candidate before its counter bump and reverse-index
        // registration exist. The `invalidate_canonical` drain serialises
        // through the OUTER `retention_gate.write()` (this publisher holds
        // the read side), so by the time a drain runs every concurrent
        // publisher has either completed its full publish_core (including
        // `canonical_index.insert`) or has not started — there is no
        // window where a candidate is in the slot but missing from the
        // canonical_index. (Lock order is `retention_gate →
        // slot.candidates.write → canonical_index shard → budget`
        // everywhere; no AB-BA.)
        let slot_ref = self
            .slots
            .entry(key.clone())
            .or_insert_with(|| Arc::new(CandidateSlot::new()));
        let deferred_victims: DeferredVictims<K>;
        {
            let mut candidates = slot_ref.candidates.write();
            let new_candidate = Arc::new(candidate);
            if let Some(pos) = candidates
                .iter()
                .position(|c| c.discriminant == new_candidate.discriminant)
            {
                // Same-view re-publish: drop the prior candidate's
                // reverse-index + ledger registrations, then re-append the
                // replacement as the newest so FIFO order reflects the
                // latest write. Occupancy is unchanged.
                let displaced = candidates.remove(pos);
                self.drain_candidate_registrations(&key, &displaced);
                candidates.push(new_candidate);
            } else {
                fresh = true;
                candidates.push(new_candidate);
                while candidates.len() > self.per_slot_cap {
                    // Oldest = front (admission order). Drain its
                    // registrations and record it as a per-slot victim.
                    let victim = candidates.remove(0);
                    self.drain_candidate_registrations(&key, &victim);
                    per_slot_evicted.push((key.clone(), victim.admission_seq));
                }
            }

            // Net live-counter delta: +1 for a fresh append, -1 per
            // per-slot victim. A replace contributes 0. Apply UNDER the
            // slot guard so the candidate's slot membership and its counter
            // contribution are one atomic step.
            let net = (fresh as i64) - (per_slot_evicted.len() as i64);
            self.apply_counter_delta(net);

            // Test-only injection point — parked AFTER the candidate has
            // been pushed into the slot and BEFORE the reverse-index
            // registration runs. With the slot write guard and the
            // retention gate read guard BOTH still held. A race test
            // arms it and, with the publisher parked here, runs
            // `invalidate_canonical` against a canonical the candidate
            // references — the `retention_gate.write()` acquire in
            // `invalidate_canonical` must block on the parked publisher's
            // read guard. `None` (the production default) is a no-op.
            #[cfg(any(test, feature = "test-support"))]
            {
                let gate = self.publish_post_push_pre_register_gate.lock().clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }

            // Register the reverse index for the freshly admitted candidate
            // under every canonical its facts name, keyed `(key, seq)` —
            // STILL under the slot guard.
            for canonical in &canonicals {
                let shard = self
                    .canonical_index
                    .entry(Arc::clone(canonical))
                    .or_insert_with(|| Mutex::new(FxHashMap::default()));
                shard.lock().insert((key.clone(), seq), ());
            }

            // Record the retention admission and capture FIFO victims for
            // deferred eviction — STILL under the slot guard, so the ledger
            // reflects the live candidate atomically with its install. The
            // victims are evicted AFTER this guard drops (re-entry into the
            // slot map would self-deadlock here).
            deferred_victims = match &self.retention_budget {
                Some(budget) => budget
                    .record_admission(seq, (key.clone(), seq))
                    .into_iter()
                    .map(|(_ledger_seq, (victim_key, victim_seq))| (victim_key, victim_seq))
                    .collect(),
                None => Vec::new(),
            };
        }

        let outcome = if !fresh {
            PublishOutcome::Replaced
        } else if per_slot_evicted.is_empty() {
            PublishOutcome::Published
        } else {
            PublishOutcome::Evicted {
                count: per_slot_evicted.len(),
            }
        };

        PublishCoreOutcome {
            outcome,
            deferred_victims,
        }
    }

    /// Evict the FIFO retention victims [`Self::publish_core`] captured.
    ///
    /// Runs AFTER the publish-core slot guard has dropped (still inside
    /// the caller's [`Self::retention_gate`] read guard). Each victim is
    /// removed identity-scoped by `(key, admission_seq)`: a concurrent
    /// same-key re-publish carrying a distinct seq survives. Cleanup runs
    /// exactly once per victim.
    pub(crate) fn evict_deferred(&self, victims: DeferredVictims<K>) {
        for (key, seq) in victims {
            self.remove_candidate_by_seq(&key, seq);
        }
    }

    /// Remove the single candidate identified by `(key, admission_seq)`,
    /// running its full cleanup exactly once: drop it from the slot,
    /// decrement the counter, drain its reverse-index registrations, and
    /// forget its retention-ledger record. Returns `true` when a candidate
    /// was removed.
    ///
    /// The slot candidate vector is mutated under the slot write guard;
    /// the empty-slot detach is a `remove_if` under the `DashMap` shard
    /// guard (the same guard `publish_core`'s `entry()` holds across its
    /// push), so a freshly published candidate is never stranded in a
    /// just-detached slot.
    fn remove_candidate_by_seq(&self, key: &K, seq: u64) -> bool {
        let Some(slot) = self.slots.get(key).map(|s| s.value().clone()) else {
            return false;
        };
        let removed = {
            let mut candidates = slot.candidates.write();
            if let Some(pos) = candidates.iter().position(|c| c.admission_seq == seq) {
                let removed = candidates.remove(pos);
                self.drain_candidate_registrations(key, &removed);
                Some(removed)
            } else {
                None
            }
        };
        if removed.is_some() {
            self.apply_counter_delta(-1);
            // Detach the slot iff it is now empty. The predicate runs
            // under the shard write guard, serialising against
            // `publish_core`'s `entry().or_insert_with()` push.
            self.slots
                .remove_if(key, |_, slot| slot.candidates.read().is_empty());
        }
        removed.is_some()
    }

    /// Drain a candidate's reverse-index registrations (one per canonical
    /// its facts name) and forget its retention-ledger record. Does NOT
    /// touch the live counter — the per-candidate counter decrement is the
    /// caller's responsibility, so a path that removes a candidate and a
    /// path that merely re-homes its registrations (a replace) each apply
    /// the right counter delta exactly once.
    fn drain_candidate_registrations(
        &self,
        key: &K,
        candidate: &Candidate<FactCandidateDiscriminant, V>,
    ) {
        let seq = candidate.admission_seq;
        for canonical in reverse_index_canonicals(candidate) {
            self.unregister_reverse_index(&canonical, key, seq);
        }
        if let Some(budget) = &self.retention_budget {
            budget.forget_seq(seq);
        }
    }

    /// Drop the `(key, seq)` registration from `canonical`'s reverse-index
    /// shard, then detach the outer shard when that removal empties its
    /// inner map. Mirrors `prune_canonical_to_keys_registration`'s
    /// shard-detach discipline.
    fn unregister_reverse_index(&self, canonical: &str, key: &K, seq: u64) {
        if let Some(shard) = self.canonical_index.get(canonical) {
            shard.lock().remove(&(key.clone(), seq));
        }
        self.canonical_index
            .remove_if(canonical, |_, m| m.lock().is_empty());
    }

    /// Apply a signed delta to the shared live counter, saturating at
    /// zero on a decrement (the counter is shared across sibling DBs, so a
    /// `store(0)` would corrupt their contributions).
    fn apply_counter_delta(&self, delta: i64) {
        match delta.cmp(&0) {
            std::cmp::Ordering::Greater => {
                self.live_counter.fetch_add(delta as u64, Ordering::Relaxed);
            }
            std::cmp::Ordering::Less => {
                let dec = (-delta) as u64;
                self.live_counter.fetch_sub(
                    dec.min(self.live_counter.load(Ordering::Relaxed)),
                    Ordering::Relaxed,
                );
            }
            std::cmp::Ordering::Equal => {}
        }
    }

    /// Drop every candidate whose facts reference `canonical_id`, draining
    /// via the per-canonical reverse index in O(K). Returns the number of
    /// candidates removed.
    ///
    /// **Gate discipline — EXCLUSIVE acquire.** Takes the retention gate
    /// `write()` guard across the whole drain. This is exclusive against
    /// every in-flight `publish_core` (which holds the gate `read()`), so
    /// by the time the `canonical_index.remove(canonical_id)` call runs
    /// here, EVERY publisher that began before this invalidator has
    /// completed its reverse-index registration. Read another way: a
    /// publisher that was racing this drain — having pushed its candidate
    /// into the slot but not yet inserted into `canonical_index` — cannot
    /// exist while the write guard is held, because the publisher's read
    /// guard blocks this write acquire. After the publisher releases its
    /// read guard, its `(key, seq)` entry IS visible in `canonical_index`,
    /// so the drain sees and removes it correctly.
    ///
    /// Read acquisition would NOT be sufficient: read+read is shared, so a
    /// publisher mid-publish could be invisible to the drain (its push has
    /// landed but its `canonical_index.insert` has not), and the drain
    /// would then return 0, leaving a candidate computed against pre-edit
    /// facts to survive the invalidation window.
    pub(crate) fn invalidate_canonical(&self, canonical_id: &str) -> usize {
        let _retention = self.retention_gate.write();
        let drained: Vec<(K, u64)> = match self.canonical_index.remove(canonical_id) {
            Some((_, mutex)) => mutex.into_inner().into_keys().collect(),
            None => return 0,
        };
        // Capture-token hook: surface the per-canonical visit count so the
        // invalidation-perf regression test can assert visited == K (NOT
        // N). Test/debug instrumentation only — gated to match the
        // capture-token module (absent in release), so the production hot
        // path pays zero cost.
        #[cfg(any(test, feature = "test-support"))]
        let visited = drained.len() as u64;
        #[cfg(any(test, feature = "test-support"))]
        crate::capture_token::with_active_capture(|t| {
            t.record_counter("invalidate_canonical_entries_visited", visited);
        });
        let mut removed = 0usize;
        for (key, seq) in drained {
            if self.remove_candidate_by_seq(&key, seq) {
                removed += 1;
            }
        }
        removed
    }

    /// Drop every candidate in every slot. Takes the retention gate WRITE
    /// guard across the whole slot-map + reverse-index + budget +
    /// counter clear, so a concurrent publish / removal (each holding the
    /// read guard) blocks until this clear completes — no publish can land
    /// a live candidate whose budget admission this reset then erases.
    /// Returns the number of candidates removed.
    pub(crate) fn invalidate_all(&self) -> usize {
        let _retention = self.retention_gate.write();
        self.clear_all_locked()
    }

    /// Inner clear used by both [`Self::invalidate_all`] and
    /// [`Self::evict_if_schema_mismatch`] — assumes the write guard is
    /// already held.
    fn clear_all_locked(&self) -> usize {
        let n: usize = self
            .slots
            .iter()
            .map(|slot| slot.value().candidates.read().len())
            .sum();
        self.slots.clear();
        self.canonical_index.clear();
        if let Some(budget) = &self.retention_budget {
            budget.clear();
        }
        self.apply_counter_delta(-(n as i64));
        n
    }

    /// Schema-eviction clear (cache-cluster schema bump). Same lock
    /// domain as [`Self::invalidate_all`]. Returns the number of
    /// candidates removed.
    pub(crate) fn evict_if_schema_mismatch(&self) -> usize {
        let _retention = self.retention_gate.write();
        self.clear_all_locked()
    }

    // -- test-only surface ---------------------------------------------

    /// Test-only direct admission bypassing the cooperative flight slot —
    /// installs a candidate and registers its reverse index exactly as the
    /// cold publish-core path. Mirrors the prior `insert_for_test`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insert_for_test(
        &self,
        key: K,
        value: V,
        signature: ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
    ) {
        let _retention = self.retention_gate.read();
        let discriminant = FactCandidateDiscriminant {
            validated_at_generation,
            facts: Arc::clone(&signature.facts),
        };
        let candidate = Candidate {
            discriminant,
            value,
            signature,
            self_root_canonicals,
            admission_seq: 0,
            validated_at_generation,
        };
        let outcome = self.publish_core(key, candidate);
        self.evict_deferred(outcome.deferred_victims);
    }

    /// Test-only: is `(key, admission_seq)` currently registered under
    /// `canonical` in the reverse index?
    #[cfg(test)]
    pub(crate) fn reverse_index_contains_for_test(
        &self,
        canonical: &str,
        key: &K,
        seq: u64,
    ) -> bool {
        self.canonical_index
            .get(canonical)
            .is_some_and(|shard| shard.lock().contains_key(&(key.clone(), seq)))
    }

    /// Test-only: is ANY candidate for `key` registered under `canonical`?
    #[cfg(test)]
    pub(crate) fn reverse_index_contains_key_for_test(&self, canonical: &str, key: &K) -> bool {
        self.canonical_index.get(canonical).is_some_and(|shard| {
            shard
                .lock()
                .keys()
                .any(|(registered_key, _)| registered_key == key)
        })
    }

    /// Test-only accessor for the lifecycle retention gate.
    #[cfg(test)]
    pub(crate) fn test_retention_gate(&self) -> &RwLock<()> {
        &self.retention_gate
    }

    /// Test-only — the number of candidates currently in slot `key`.
    #[cfg(test)]
    pub(crate) fn slot_len_for_test(&self, key: &K) -> usize {
        self.slots
            .get(key)
            .map(|s| s.value().candidates.read().len())
            .unwrap_or(0)
    }

    /// Test-only — the number of admission records currently in the
    /// retention ledger (budgeted form only).
    #[cfg(test)]
    pub(crate) fn retention_tracked_len(&self) -> usize {
        self.retention_budget
            .as_ref()
            .map(|b| b.tracked_len())
            .unwrap_or(0)
    }

    /// Test-only — number of distinct outer shards in the reverse index.
    #[cfg(test)]
    pub(crate) fn canonical_index_shard_count_for_test(&self) -> usize {
        self.canonical_index.len()
    }

    /// Test-only read of the shared live counter.
    #[cfg(test)]
    pub(crate) fn live_counter_for_test(&self) -> u64 {
        self.live_counter.load(Ordering::Relaxed)
    }
}

/// The set of canonicals a candidate's reverse-index registration covers:
/// the union of its facts' canonicals (`signature.canonical_ids()`) and
/// its strict `self_root_canonicals`. A per-canonical invalidation of ANY
/// of these must reach the candidate. The self-roots are included because
/// a candidate with no facts (an empty signature) still self-roots its
/// keyed canonical — e.g. the imported-registry keyed canonical — and that
/// self-root must be drainable.
fn reverse_index_canonicals<V>(
    candidate: &Candidate<FactCandidateDiscriminant, V>,
) -> Vec<Arc<str>> {
    let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
    let mut out: Vec<Arc<str>> = Vec::new();
    for canonical in candidate.signature.canonical_ids() {
        if seen.insert(Arc::clone(&canonical)) {
            out.push(canonical);
        }
    }
    for canonical in candidate.self_root_canonicals.iter() {
        if seen.insert(Arc::clone(canonical)) {
            out.push(Arc::clone(canonical));
        }
    }
    out
}

#[cfg(test)]
#[path = "candidate_store_tests.rs"]
mod tests;
