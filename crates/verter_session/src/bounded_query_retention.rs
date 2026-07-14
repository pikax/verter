//! Bounded query-identity retention substrate.
//!
//! Query-identity caches store entries whose effective identity carries
//! self-version state (an owner whole-hash, a `DeclIdentity` embedding a
//! file whole-hash, a content-derived `SemanticNodeId`). Each distinct
//! content edit of an owner appends a fresh entry, so without a routine
//! reclamation path those caches grow monotonically with the edit count
//! in a long-lived session.
//!
//! This module is the single shared substrate that bounds that whole
//! class. It exposes two cooperating pieces tuned per cache:
//!
//! - [`GlobalRetentionBudget`] — a process-cheap, insertion-ordered
//!   (FIFO) total-size budget. A cache records each admitted entry's key
//!   together with its unique admission sequence number; once the
//!   recorded count exceeds the cache's configured cap the budget hands
//!   back the oldest `(seq, key)` victims so the caller can evict them.
//!   The victim carries its admission `seq` so the caller can scope the
//!   map removal to that exact admission — a same-key re-publish racing
//!   the eviction carries a different `seq` and must survive. Caches
//!   whose backing map is
//!   owned by the cooperative-admission primitive
//!   ([`crate::component_meta_caches::MaterializeStructureDb`],
//!   [`crate::component_meta_caches::RefCycleResultDb`], the
//!   [`crate::semantic_query_memo::SemanticGraphStore`] memo + node
//!   arena) embed a `GlobalRetentionBudget` and drive eviction from
//!   their write-side `post_publish` hook.
//!
//! - [`BoundedCandidateMap`] — a query-identity slot map whose outer key
//!   is **content-free**; each slot holds a bounded candidate list and a
//!   distinct concurrent version is a candidate inside the slot. A fifth
//!   candidate in a four-deep slot evicts the slot's oldest candidate; a
//!   `GlobalRetentionBudget` additionally caps the total candidate count
//!   across all slots. [`crate::component_meta_result_db::ComponentMetaResultDb`]
//!   is built on this.
//!
//! ## Eviction policy
//!
//! Eviction is **stale-first when cheaply detectable, then FIFO**.
//! Insertion sequence numbers (a shared monotonic counter) provide the
//! FIFO order; the substrate never does read-time LRU bookkeeping, so a
//! warm read is a shared borrow with no atomic write. Evicting a *valid*
//! entry is permitted — it only forces a recompute, never an incorrect
//! result. Cleanup runs write-side (on insert).
//!
//! ## Concurrency
//!
//! A reader clones the candidate `Arc` out of the slot before
//! validating it, so a concurrent removal never invalidates an in-flight
//! reader's borrow. Removal is keyed by candidate identity (the
//! insertion sequence number, unique per admitted candidate), so a
//! concurrent re-admission under the same discriminant is never mistaken
//! for the candidate being evicted.
//!
//! ## Single write-side consistency domain (rule)
//!
//! Every budgeted cache must have exactly one write-side consistency
//! domain: either the map + budget + reverse index are mutated under
//! one exclusive lock, or every gap is closed by BOTH atomic same-key
//! admission AND identity-scoped removal. New budgeted caches must
//! prefer structural (single-lock) serialization.
//!
//! [`BoundedCandidateMap::admit`] follows the single-lock form: the
//! slot mutation, the `forget_seq` of the removed candidate seqs, and
//! the `record_admission` of the newly-live candidate all run inside
//! one continuously-held slot `Mutex` critical section, so the slot map
//! and the [`GlobalRetentionBudget`] ledger move as one atomic
//! write-side step. The `retention_gate` is a coarse reset fence only —
//! a shared read guard does not serialise two admits of the same
//! content-free slot; the slot `Mutex` does. Global-budget victim
//! eviction runs after the slot lock is released (lock order:
//! `retention_gate.read → DashMap shard/slot → slot Mutex → budget
//! Mutex`, victim slot lock taken last — no AB-BA).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

/// Process-wide monotonic allocator for candidate / entry insertion
/// sequence numbers. The sequence number is the FIFO eviction order and
/// doubles as a per-admission identity that survives a same-key
/// re-admission.
static RETENTION_SEQ: AtomicU64 = AtomicU64::new(1);

/// Allocate the next insertion sequence number. Strictly monotonic and
/// never reused for the lifetime of the process.
#[must_use]
pub fn next_retention_seq() -> u64 {
    RETENTION_SEQ.fetch_add(1, Ordering::Relaxed)
}

// ===========================================================================
// GlobalRetentionBudget — shared FIFO total-size budget
// ===========================================================================

/// Insertion-ordered total-size budget shared by every bounded
/// query-identity cache.
///
/// A cache calls [`Self::record_admission`] for each entry it admits,
/// passing the entry's map key and its insertion sequence number. The
/// budget keeps a FIFO ledger of admitted `(seq, key)` pairs; when the
/// ledger exceeds `cap` it returns the oldest `(seq, key)` victims so the
/// caller can evict them from its own map (running whatever reverse-index
/// / counter cleanup the cache needs).
///
/// **Victims carry their admission identity.** A FIFO victim is a
/// `(seq, key)` pair, not a bare key. A concurrent same-key re-publish
/// can overwrite the map slot under `key` with a *fresh* entry (a
/// distinct `seq`) before the caller acts on the victim. A bare-key
/// removal would then evict that fresh entry and strand its still-live
/// ledger record, so the cache grows past `cap`. A correct victim
/// consumer scopes the removal to `victim_seq` — it removes the map
/// entry under `victim_key` ONLY IF that entry's stored `admission_seq`
/// equals `victim_seq`. The single exception is a consumer whose map and
/// budget are mutated under an exclusive lock that also serialises every
/// `record_admission` of the same key (e.g. a `&mut self` store): with
/// no concurrent re-admission possible a bare-key removal is sound, and
/// that consumer must name the serialising lock.
///
/// The ledger holds `(seq, key)` pairs only — never payloads — so it is
/// cheap. A cache that removes an entry through its own invalidation path
/// keeps the ledger consistent so it does not later hand back a key whose
/// entry is already gone:
///
/// - [`Self::forget_seq`] — drops exactly one admission by its unique
///   sequence number. Removal-identity-safe: sound under a shared read
///   guard because it can never delete a concurrent writer's fresh,
///   distinctly-seq'd admission of the same key. Every identity-scoped
///   cache uses this.
/// - [`Self::forget_key_under_exclusive_lock`] — a KEY-WIDE removal that
///   drops every admission for a key. Sound ONLY when the caller holds
///   the cache's exclusive write lock so no concurrent admission can
///   race; see that method's contract.
pub struct GlobalRetentionBudget<K> {
    /// FIFO ledger of admitted entries, oldest at the front.
    ledger: parking_lot::Mutex<VecDeque<(u64, K)>>,
    /// Maximum number of live admitted entries retained. Exceeding it on
    /// an admission returns the oldest keys for eviction.
    cap: usize,
}

impl<K> GlobalRetentionBudget<K>
where
    K: Clone + PartialEq,
{
    /// Construct a budget with the given total cap. A `cap` of `0` is
    /// clamped to `1` so a cache always retains at least its newest
    /// entry.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            ledger: parking_lot::Mutex::new(VecDeque::new()),
            cap: cap.max(1),
        }
    }

    /// The configured total cap.
    #[must_use]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Record one freshly-admitted entry. Returns the oldest `(seq, key)`
    /// victims that must now be evicted to bring the ledger back within
    /// `cap` (empty when the cache is still within budget).
    ///
    /// Each victim carries its own admission `seq` — the caller scopes
    /// the map removal to that `seq` (remove only when the entry under
    /// `victim_key` still carries `admission_seq == victim_seq`) so a
    /// concurrent same-key re-publish, which carries a distinct `seq`,
    /// survives. See the struct-level "Victims carry their admission
    /// identity" contract.
    ///
    /// The returned victims are removed from the ledger immediately, so a
    /// caller that evicts them keeps the ledger consistent with its map.
    #[must_use]
    pub fn record_admission(&self, seq: u64, key: K) -> Vec<(u64, K)> {
        let mut ledger = self.ledger.lock();
        ledger.push_back((seq, key));
        let mut evict = Vec::new();
        while ledger.len() > self.cap {
            if let Some(victim) = ledger.pop_front() {
                evict.push(victim);
            }
        }
        evict
    }

    /// Drop EVERY ledger entry for `key`, regardless of admission seq.
    ///
    /// **Removal-identity hazard — read this before calling.** This is a
    /// KEY-WIDE removal: it deletes every `(seq, key)` pair the ledger
    /// holds for `key`, including a record admitted by a *different*,
    /// concurrent writer. If a caller runs this under only a shared read
    /// guard while another thread is free to `record_admission` the same
    /// key, the key-wide removal can delete that concurrent writer's
    /// FRESH admission while the writer's fresh map entry survives —
    /// stranding a live entry invisible to FIFO eviction so the cache
    /// grows past its cap. That is the bug class
    /// [`GlobalRetentionBudget::forget_seq`] exists to avoid.
    ///
    /// **Precondition (caller-enforced):** the caller MUST hold the
    /// cache's exclusive write lock — the same lock domain every
    /// `record_admission` of this budget runs under — for the whole
    /// duration of this call, so no concurrent admission of `key` can
    /// race. A caller that only holds a shared read guard MUST use
    /// `forget_seq` with the removed entry's own admission seq instead.
    ///
    /// The single in-tree caller is the `SemanticGraphStore` family
    /// memo's per-canonical drain, which runs inside the `entries`
    /// `Mutex` hold — the exact lock domain `record_family_admission_locked`
    /// records under and `invalidate_all` clears under. Identity-scoped
    /// caches (`BoundedCandidateMap`, the
    /// `component_meta_caches` DBs) use `forget_seq` and never call this.
    pub fn forget_key_under_exclusive_lock(&self, key: &K) {
        let mut ledger = self.ledger.lock();
        ledger.retain(|(_, k)| k != key);
    }

    /// Drop the single ledger entry identified by `seq`. Used when an
    /// individual candidate / entry (not a whole key) is evicted — the
    /// sequence number is unique per admission, so this removes exactly
    /// that admission's ledger record and never a re-admission under the
    /// same key. This is the removal-identity-safe primitive: it is
    /// sound under only a shared read guard because it can never delete
    /// a concurrent writer's fresh, distinctly-seq'd admission.
    pub fn forget_seq(&self, seq: u64) {
        let mut ledger = self.ledger.lock();
        ledger.retain(|(s, _)| *s != seq);
    }

    /// Clear the whole ledger. Called on a project-generation reset that
    /// drops every cache entry at once.
    pub fn clear(&self) {
        self.ledger.lock().clear();
    }

    /// Number of entries currently tracked. Test-only diagnostics.
    #[cfg(test)]
    #[must_use]
    pub fn tracked_len(&self) -> usize {
        self.ledger.lock().len()
    }
}

/// Default total cap for a `GlobalRetentionBudget` constructed via
/// [`Default`]. Sized for a query memo (`SemanticGraphStore`); caches
/// that want a different cap construct the budget with an explicit
/// [`GlobalRetentionBudget::new`].
pub const DEFAULT_BUDGET_CAP: usize = 4096;

impl<K> Default for GlobalRetentionBudget<K>
where
    K: Clone + PartialEq,
{
    fn default() -> Self {
        Self::new(DEFAULT_BUDGET_CAP)
    }
}

// ===========================================================================
// BoundedCandidateMap — content-free slot key, bounded candidate list
// ===========================================================================

/// Default per-slot candidate cap. Concurrent overlay variants of the
/// same query identity coexist as candidates inside one slot; four
/// covers the `{current, previous, two concurrent overlays}` working
/// set. Per the multi-candidate cache model (architecture rule R20).
pub const DEFAULT_CANDIDATE_CAP: usize = 4;

/// One stored candidate inside a [`BoundedCandidateMap`] slot.
///
/// `discriminant` is the self-version state the slot key intentionally
/// omits (e.g. an owner whole-hash). `seq` is the FIFO insertion order
/// and the candidate's removal identity. `value` is the payload the
/// caller stores — typically a payload `Arc` plus its read-set / fact
/// signature.
pub struct RetentionCandidate<D, V> {
    /// Self-version discriminant carried by the candidate, not the slot
    /// key. Two candidates in one slot differ by this value.
    pub discriminant: D,
    /// Monotonic insertion sequence — FIFO order and removal identity.
    pub seq: u64,
    /// Caller payload.
    pub value: V,
}

/// Outcome of a [`BoundedCandidateMap::admit`] call.
///
/// The exact net live-candidate-count delta of the admission is
/// `(fresh as i64) - (evicted as i64)`. A caller maintaining an external
/// live counter applies that delta — `fetch_add` / `fetch_sub` — rather
/// than re-deriving an absolute snapshot, which a concurrent admission
/// could clobber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmitOutcome {
    /// `true` when the admission appended a NEW candidate; `false` when
    /// it replaced an existing same-discriminant candidate in place. A
    /// replace leaves live occupancy unchanged.
    pub fresh: bool,
    /// Number of candidates evicted by this admission (per-slot cap +
    /// global budget). Each eviction lowers live occupancy by one.
    pub evicted: usize,
}

/// A query-identity slot — a bounded, insertion-ordered list of
/// candidates. Held behind an `Arc` in the outer map; the candidate
/// vector itself is behind a `Mutex` so admissions and stale reaping
/// serialise per slot while the outer map stays lock-free per shard.
pub struct CandidateSlot<D, V> {
    candidates: parking_lot::Mutex<Vec<Arc<RetentionCandidate<D, V>>>>,
}

impl<D, V> CandidateSlot<D, V> {
    fn new() -> Self {
        Self {
            candidates: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// Snapshot the slot's candidates. The returned `Arc`s are owned by
    /// the caller, so a concurrent removal cannot invalidate them — a
    /// reader validates the snapshot without holding the slot lock.
    #[cfg(test)]
    #[must_use]
    pub fn snapshot(&self) -> Vec<Arc<RetentionCandidate<D, V>>> {
        self.candidates.lock().clone()
    }
}

/// Content-free query-identity slot map with bounded per-slot candidate
/// lists and a shared global total-size budget.
///
/// The outer key `K` is content-free (it carries query / owner / options
/// / env identity but no content version). Concurrent versions of the
/// same query identity are candidates inside one slot, capped at
/// `per_slot_cap`. A `GlobalRetentionBudget` caps the total candidate
/// count across all slots; both caps evict oldest-first.
///
/// ## Map / budget lock domain
///
/// The slot map and its `GlobalRetentionBudget` are two structures that
/// must stay consistent: the budget's ledger tracks which live slot
/// candidates exist so it can FIFO-evict the oldest past the cap. Every
/// mutation that changes BOTH — `admit`, `evict_candidate`,
/// `evict_slot`, `retain_slots` — runs under a shared `retention_gate`
/// read guard, and `clear` runs under the `retention_gate` write guard.
/// A concurrent `admit` and `clear` therefore cannot interleave their
/// map and budget steps, so the budget never strands a record for a
/// candidate the map dropped (or vice versa) — the cache bound stays
/// guaranteed across a project-generation reset. `DashMap` stays for
/// hot-path per-shard concurrency; the gate is a coarse reset fence,
/// not a hot-path serialiser, so concurrent admits to different keys
/// still run in parallel under the shared read guard.
pub struct BoundedCandidateMap<K, D, V> {
    slots: DashMap<K, Arc<CandidateSlot<D, V>>>,
    budget: GlobalRetentionBudget<(K, u64)>,
    per_slot_cap: usize,
    /// Lifecycle gate. Mutations that touch both `slots` and `budget`
    /// take the read guard for the whole map+budget mutation; `clear`
    /// takes the write guard for the whole map+budget clear. See the
    /// struct-level "Map / budget lock domain" docs.
    retention_gate: parking_lot::RwLock<()>,
    /// Test-only injection point inside [`Self::clear`], parked between
    /// the slot-map clear and the budget clear. A test arms it to drive
    /// a concurrent `admit` deterministically into the gap and assert
    /// the gate closes the desync. Per-instance (not process-global) so
    /// arming it on one map never parks an unrelated concurrent test's
    /// `clear`. Absent from release builds — the production reset path
    /// is unchanged.
    #[cfg(any(test, debug_assertions))]
    clear_midpoint_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::admit`], parked AFTER
    /// the slot push and the budget `record_admission` complete but
    /// before `admit` returns. Pairs with `clear_midpoint_gate` to make
    /// the map/budget desync race deterministic: a test confirms the
    /// admit's update has fully landed (admitter parked here) before it
    /// releases a concurrently-parked `clear`. Per-instance; absent from
    /// release builds.
    #[cfg(any(test, debug_assertions))]
    admit_post_record_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::admit`], parked AFTER
    /// the slot mutation + removed-seq `forget_seq` but BEFORE the
    /// global-budget `record_admission` — with the slot `Mutex` AND the
    /// `DashMap` shard guard for the admitted key STILL held. An
    /// admit-vs-admit race test arms it to pin one admit there and prove
    /// a concurrent admit of the same slot cannot record between this
    /// admit's slot mutation and its `record_admission`. Per-instance;
    /// absent from release builds.
    #[cfg(any(test, debug_assertions))]
    admit_pre_budget_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

impl<K, D, V> BoundedCandidateMap<K, D, V>
where
    K: Eq + std::hash::Hash + Clone,
    D: PartialEq + Clone,
{
    /// Construct with an explicit per-slot candidate cap and global
    /// total-candidate cap. Both are clamped to at least `1`.
    #[must_use]
    pub fn with_caps(per_slot_cap: usize, global_cap: usize) -> Self {
        Self {
            slots: DashMap::new(),
            budget: GlobalRetentionBudget::new(global_cap),
            per_slot_cap: per_slot_cap.max(1),
            retention_gate: parking_lot::RwLock::new(()),
            #[cfg(any(test, debug_assertions))]
            clear_midpoint_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            admit_post_record_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            admit_pre_budget_gate: parking_lot::Mutex::new(None),
        }
    }

    /// Test-only driver: arm the [`Self::clear`] injection point with
    /// `barrier`. The next `clear` on **this map** calls `barrier.wait()`
    /// TWICE between the slot-map clear and the budget clear (with the
    /// `retention_gate` write guard still held): the test's first
    /// `wait()` confirms `clear` is pinned mid-flight, its second
    /// `wait()` releases it. The returned guard disarms the injection
    /// point on drop.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub fn test_arm_clear_midpoint_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> ClearMidpointGateGuard<'_> {
        *self.clear_midpoint_gate.lock() = Some(barrier);
        ClearMidpointGateGuard {
            gate: &self.clear_midpoint_gate,
        }
    }

    /// Test-only driver: arm the [`Self::admit`] injection point with
    /// `barrier`. The next `admit` on **this map** calls `barrier.wait()`
    /// TWICE after its slot push + budget admission complete but before
    /// it returns (with the `retention_gate` read guard still held): the
    /// test's first `wait()` confirms `admit` is pinned mid-flight, its
    /// second `wait()` releases it. The returned guard disarms the
    /// injection point on drop.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub fn test_arm_admit_post_record_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> AdmitPostRecordGateGuard<'_> {
        *self.admit_post_record_gate.lock() = Some(barrier);
        AdmitPostRecordGateGuard {
            gate: &self.admit_post_record_gate,
        }
    }

    /// Test-only driver: arm the pre-budget [`Self::admit`] injection
    /// point with `barrier`. The next `admit` on **this map** calls
    /// `barrier.wait()` TWICE after its slot mutation + removed-seq
    /// `forget_seq` but BEFORE the global-budget `record_admission` —
    /// with the slot `Mutex` AND the `DashMap` shard guard for the
    /// admitted key STILL held: the test's first `wait()` confirms
    /// `admit` is pinned mid-flight, its second `wait()` releases it.
    /// An admit-vs-admit race test uses this to prove a concurrent
    /// admit of the same slot cannot record between the parked admit's
    /// slot mutation and its `record_admission`. The returned guard
    /// disarms the injection point on drop.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub fn test_arm_admit_pre_budget_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> AdmitPreBudgetGateGuard<'_> {
        *self.admit_pre_budget_gate.lock() = Some(barrier);
        AdmitPreBudgetGateGuard {
            gate: &self.admit_pre_budget_gate,
        }
    }

    /// Test-only accessor for the lifecycle [`retention_gate`]. A race
    /// test parks a mutator mid-flight (via an injection point) and
    /// uses `try_read()` / `try_write()` on this gate to assert,
    /// deterministically, that the in-flight mutator has engaged the
    /// gate against the opposing access mode.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub fn test_retention_gate(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
    }

    /// Test-only — `true` when the `DashMap` shard backing slot `key` is
    /// currently write-locked by another thread (a `try_get` that
    /// returns `Locked`).
    ///
    /// An admit-vs-admit race test parks an `admit` at the pre-budget
    /// injection point and probes this: with the single-lock-domain
    /// `admit`, the parked admit still holds the `entry()` shard write
    /// guard for `key` across its `record_admission`, so this returns
    /// `true`. A pre-fix `admit` that dropped the slot lock + shard
    /// guard before `record_admission` leaves the shard unlocked, so
    /// this returns `false` — the deterministic discriminator that the
    /// slot mutation and the budget admission share one critical
    /// section.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub fn test_key_shard_locked(&self, key: &K) -> bool {
        matches!(
            self.slots.try_get(key),
            dashmap::try_result::TryResult::Locked
        )
    }

    /// The per-slot candidate cap.
    #[must_use]
    pub fn per_slot_cap(&self) -> usize {
        self.per_slot_cap
    }

    /// The global total-candidate cap.
    #[must_use]
    pub fn global_cap(&self) -> usize {
        self.budget.cap()
    }

    /// Total live candidate count across every slot. This is the cache's
    /// authoritative occupancy — the number the bound-proof asserts on.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.slots
            .iter()
            .map(|slot| slot.value().candidates.lock().len())
            .sum()
    }

    /// Snapshot the candidates of slot `key`. Empty when the slot is
    /// absent. The returned `Arc`s outlive any concurrent removal.
    /// Test-only enumeration accessor.
    #[cfg(test)]
    #[must_use]
    pub fn slot_candidates(&self, key: &K) -> Vec<Arc<RetentionCandidate<D, V>>> {
        match self.slots.get(key) {
            Some(slot) => slot.value().snapshot(),
            None => Vec::new(),
        }
    }

    /// Look up the candidate in slot `key` whose discriminant matches
    /// `discriminant`. Returns an owned `Arc` so the caller can validate
    /// it after the slot lock is released.
    #[must_use]
    pub fn get_candidate(
        &self,
        key: &K,
        discriminant: &D,
    ) -> Option<Arc<RetentionCandidate<D, V>>> {
        let slot = self.slots.get(key)?;
        let found = slot
            .value()
            .candidates
            .lock()
            .iter()
            .find(|c| &c.discriminant == discriminant)
            .cloned();
        found
    }

    /// Admit a candidate into slot `key`.
    ///
    /// A candidate already present under the same `discriminant` is
    /// replaced in place (a re-publish of the same version refreshes the
    /// payload without growing the slot). Otherwise the candidate is
    /// appended; if the slot then exceeds `per_slot_cap` its oldest
    /// candidate is evicted. The global budget is consulted last and may
    /// evict an oldest candidate from a *different* slot.
    ///
    /// Returns an [`AdmitOutcome`] reporting whether this admission added
    /// a fresh candidate (vs. an in-place replace) and how many
    /// candidates it evicted. The exact net live-count delta is
    /// therefore `(fresh as i64) - (evicted as i64)`, which a caller uses
    /// to maintain an external live counter by net-delta accounting
    /// rather than an unsynchronised absolute snapshot.
    ///
    /// ## Single-lock-domain write side
    ///
    /// The slot mutation AND the budget admission for this admit run
    /// inside ONE continuously-held slot-lock critical section. The
    /// outer slot key is content-free, so two concurrent cold admits
    /// legitimately target the same slot; the `retention_gate` is only
    /// a coarse reset fence (a shared *read* guard — it does not
    /// serialise two admits). If the slot `Mutex` were dropped between
    /// the slot mutation and `record_admission`, a later admit could
    /// replace this admit's candidate and `forget_seq` it before this
    /// admit recorded — this admit would then record a ghost seq for an
    /// already-evicted candidate, and under a small global cap that
    /// ghost can evict a live entry. Holding the slot lock across the
    /// slot mutation, the `forget_seq` of the removed seqs, AND the
    /// `record_admission` of the newly-live candidate makes the map and
    /// the budget one atomic write-side step.
    ///
    /// The global-budget victims returned by `record_admission` are
    /// evicted AFTER the slot lock (and the `DashMap` shard guard) are
    /// released — `remove_candidate_by_seq` re-enters `self.slots` for a
    /// victim that may live on this same shard, so it must not run under
    /// the shard guard. The lock order is `retention_gate.read →
    /// DashMap shard/slot → slot Mutex → budget Mutex`; the victim slot
    /// lock is taken only after this admit's slot lock is released, so
    /// there is no `slot-A → … → slot-B` AB-BA cycle.
    ///
    /// **Record only for a live admission.** `record_admission` runs
    /// only when the newly-admitted candidate is still present in the
    /// slot after the per-slot cap is applied. Recording a candidate the
    /// per-slot cap immediately evicted would strand a ledger record for
    /// a candidate the map does not hold. (A `per_slot_cap` of at least
    /// `1` plus the monotonic `seq` means an appended candidate — the
    /// largest seq — is never the per-slot cap's oldest-by-seq victim,
    /// so this guard is structural insurance against a future cap or
    /// eviction-policy change rather than a path reachable today.)
    ///
    /// **Slot-detach safety.** The candidate push happens while the
    /// `DashMap` shard write guard for `key` (the `RefMut` returned by
    /// `entry().or_insert_with()`) is still held. An empty-slot reaper
    /// ([`Self::remove_candidate_by_seq`]'s `remove_if`) acquires the
    /// same shard write lock to test-and-detach a slot, so it can never
    /// interleave between "slot observed empty" and "this admit pushed
    /// its candidate" — the slot the admitter populates is always still
    /// attached when the shard guard is released, and the published
    /// candidate is always reachable by later reads / `live_count`.
    pub fn admit(&self, key: K, discriminant: D, value: V) -> AdmitOutcome {
        // Hold the retention-gate read guard across the WHOLE map +
        // budget mutation: the slot push, the per-slot eviction, the
        // budget `forget_seq` cleanup, the global-budget admission, and
        // the victim eviction. A concurrent `clear` takes the write
        // guard, so it cannot interleave its slot-map clear and budget
        // clear with this admit's update — the budget never strands a
        // record for a candidate this admit landed. The guard is a
        // coarse reset fence: it does NOT serialise two admits of the
        // same slot (that is the slot `Mutex`'s job below); concurrent
        // admits to other keys still run in parallel under the shared
        // read guard (each on its own `DashMap` shard).
        let _retention = self.retention_gate.read();
        let seq = next_retention_seq();

        let mut evicted = 0usize;
        // `true` when this admission appended a NEW candidate; `false`
        // when it replaced an existing same-discriminant candidate in
        // place. A replace does not change live occupancy.
        let mut fresh = false;
        // Global-budget victims to evict AFTER the slot lock is released
        // — see the "single-lock-domain write side" docs.
        let over_budget;
        {
            // Hold the shard write guard for `key` across the candidate
            // push: a concurrent reaper's `remove_if`-empty needs this
            // same guard, so it cannot detach the slot mid-admit.
            let slot_ref = self
                .slots
                .entry(key.clone())
                .or_insert_with(|| Arc::new(CandidateSlot::new()));
            // The slot `Mutex` is held continuously across the slot
            // mutation, the `forget_seq` of the removed seqs, AND the
            // `record_admission` of the newly-live candidate — so a
            // concurrent admit of the same slot cannot replace+forget
            // this admit's candidate before this admit records it.
            let mut candidates = slot_ref.candidates.lock();
            // Seqs of candidates this admit removed (a replaced
            // candidate or per-slot-cap victims) — forgotten from the
            // budget below, still under the slot lock.
            let mut forget_seqs: Vec<u64> = Vec::new();
            if let Some(existing) = candidates
                .iter_mut()
                .find(|c| c.discriminant == discriminant)
            {
                // Same-version re-publish: replace in place. The ledger
                // still tracks the prior admission's seq — drop it and
                // record the fresh one so FIFO order reflects the latest
                // write.
                forget_seqs.push(existing.seq);
                *existing = Arc::new(RetentionCandidate {
                    discriminant,
                    seq,
                    value,
                });
            } else {
                fresh = true;
                candidates.push(Arc::new(RetentionCandidate {
                    discriminant,
                    seq,
                    value,
                }));
                // Per-slot cap: evict oldest-by-seq until within cap.
                while candidates.len() > self.per_slot_cap {
                    // Oldest = smallest seq.
                    if let Some((idx, _)) = candidates.iter().enumerate().min_by_key(|(_, c)| c.seq)
                    {
                        let removed = candidates.remove(idx);
                        forget_seqs.push(removed.seq);
                        evicted += 1;
                    } else {
                        break;
                    }
                }
            }
            // Whether the candidate this admit allocated `seq` for is
            // still resident after the per-slot cap was applied. A
            // replace leaves the new candidate in place; an append
            // leaves it in place unless the per-slot cap immediately
            // evicted it (structurally unreachable while `per_slot_cap
            // >= 1` — see the doc comment).
            let new_candidate_live = candidates.iter().any(|c| c.seq == seq);
            // Forget the removed seqs — STILL holding the slot lock, so
            // a concurrent same-slot admit cannot interleave between
            // this forget and the `record_admission` below.
            for s in forget_seqs {
                self.budget.forget_seq(s);
            }
            // Test-only injection point — parked AFTER the slot mutation
            // and the removed-seq `forget_seq` but BEFORE
            // `record_admission`, with the slot `Mutex` AND the
            // `DashMap` shard guard for `key` STILL held. A race test
            // arms it with a barrier and calls `wait()` TWICE: the first
            // `wait()` rendezvous lets the test observe this admit is
            // pinned here; the second `wait()` releases it. Because the
            // slot lock + shard guard are held, a concurrent admit of
            // the same slot cannot record between this admit's slot
            // mutation and its `record_admission` — the test drives that
            // serialisation. `None` (the production default) is a no-op.
            #[cfg(any(test, debug_assertions))]
            {
                let gate = self.admit_pre_budget_gate.lock().clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
            // Global budget: record this admission ONLY when its
            // candidate is still live, then collect the oldest entries
            // past the global cap. STILL holding the slot lock — the
            // map mutation and the budget admission are one atomic
            // write-side step. The budget ledger seq and the candidate
            // seq embedded in the budget key are the same value, so
            // `remove_candidate_by_seq` (identity-scoped) removes a
            // victim slot candidate ONLY when its `seq` matches.
            over_budget = if new_candidate_live {
                self.budget.record_admission(seq, (key.clone(), seq))
            } else {
                Vec::new()
            };
            // Release the candidate lock then the shard guard before the
            // global-budget victim eviction: `remove_candidate_by_seq`
            // re-enters `self.slots` for a victim that may live on this
            // same shard, which would deadlock if the shard guard were
            // still held.
            drop(candidates);
            drop(slot_ref);
        }

        // Evict the global-budget victims. Runs AFTER the slot lock is
        // released (lock order: this admit's slot lock is dropped before
        // any victim slot lock is taken — no AB-BA). Between the slot
        // push and this trim a concurrent `live_count` may transiently
        // observe one candidate over the global cap (this admit's push
        // landed, its over-budget victim not yet removed). That is a
        // momentary overcount of a *bounded* quantity (at most one
        // admit's worth per concurrent admitter), not unbounded growth —
        // the budget ledger is authoritative and the victim is removed
        // before `admit` returns. A future reader should not treat the
        // window as a bug.
        for (_ledger_seq, (victim_key, victim_seq)) in over_budget {
            if self.remove_candidate_by_seq(&victim_key, victim_seq) {
                evicted += 1;
            }
        }
        // Test-only injection point — parked AFTER the slot push and
        // budget admission have fully landed but BEFORE `admit` returns,
        // so the `retention_gate` read guard is still held. A race test
        // arms it with a barrier and calls `wait()` TWICE: the first
        // `wait()` rendezvous lets the test observe that `admit` is
        // pinned here (read guard engaged); the second `wait()` releases
        // `admit` to drop the guard and return. `None` (the production
        // default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.admit_post_record_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        AdmitOutcome { fresh, evicted }
    }

    /// Remove the single candidate identified by `(key, seq)`. Returns
    /// `true` when a candidate was removed. An empty slot is dropped.
    fn remove_candidate_by_seq(&self, key: &K, seq: u64) -> bool {
        let Some(slot) = self.slots.get(key) else {
            return false;
        };
        let removed = {
            let mut candidates = slot.value().candidates.lock();
            if let Some(idx) = candidates.iter().position(|c| c.seq == seq) {
                candidates.remove(idx);
                true
            } else {
                false
            }
        };
        drop(slot);
        if removed {
            // Drop the slot if it is still empty. `remove_if` holds the
            // shard write lock while it runs the emptiness predicate and
            // detaches the slot; [`Self::admit`] holds that same shard
            // write guard across its candidate push. The two therefore
            // serialise: a `remove_if` that observes the slot empty has
            // exclusive shard access, so no in-flight admit can be
            // mid-push into this slot — and a `remove_if` racing an
            // admit either runs first (predicate sees the slot, which
            // the admit then repopulates and re-checks is irrelevant —
            // the slot was non-empty so detach is skipped) or runs after
            // (predicate sees the admit's candidate, detach skipped). A
            // freshly published candidate is never stranded in a
            // detached slot.
            self.slots
                .remove_if(key, |_, slot| slot.candidates.lock().is_empty());
        }
        removed
    }

    /// Remove a candidate by its identity (`seq`) from slot `key`,
    /// running the budget cleanup. Used by callers that proactively reap
    /// a candidate they found stale on read. Returns `true` when a
    /// candidate was removed.
    pub fn evict_candidate(&self, key: &K, seq: u64) -> bool {
        // Read guard across the slot removal AND the budget `forget_seq`
        // — a concurrent `clear` (write guard) cannot interleave its
        // map/budget clear with this removal's two steps.
        let _retention = self.retention_gate.read();
        let removed = self.remove_candidate_by_seq(key, seq);
        if removed {
            self.budget.forget_seq(seq);
        }
        removed
    }

    /// Drop every candidate in slot `key` (all versions). Returns the
    /// number removed. Test-only single-slot drain — production
    /// per-owner invalidation goes through [`Self::retain_slots`].
    #[cfg(test)]
    pub fn evict_slot(&self, key: &K) -> usize {
        // Read guard across the slot removal AND the per-candidate
        // budget `forget_seq` — consistent with `admit` / `clear`.
        let _retention = self.retention_gate.read();
        let Some((_, slot)) = self.slots.remove(key) else {
            return 0;
        };
        let drained = std::mem::take(&mut *slot.candidates.lock());
        for c in &drained {
            self.budget.forget_seq(c.seq);
        }
        drained.len()
    }

    /// Drop every slot and every candidate. Returns the number of
    /// candidates removed. Used on a project-generation reset.
    ///
    /// Takes the `retention_gate` write guard across BOTH the slot-map
    /// clear and the budget clear. A concurrent `admit` / `evict_*` /
    /// `retain_slots` holds the read guard, so it blocks until this
    /// clear completes — no admit can land a candidate in `slots` whose
    /// budget record this clear then erases (or land a budget record
    /// for a candidate this clear already dropped).
    pub fn clear(&self) -> usize {
        let _retention = self.retention_gate.write();
        let mut removed = 0usize;
        for slot in self.slots.iter() {
            removed += slot.value().candidates.lock().len();
        }
        self.slots.clear();
        // Test-only injection point — parked between the slot-map clear
        // and the budget clear, with the `retention_gate` write guard
        // still held. A race test arms it with a barrier and calls
        // `wait()` TWICE: the first `wait()` rendezvous lets the test
        // observe that `clear` is pinned here (write guard engaged); the
        // second `wait()` releases `clear` to finish. `None` (the
        // production default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.clear_midpoint_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.budget.clear();
        removed
    }

    /// Retain only the slots whose key satisfies `keep`; every candidate
    /// of a dropped slot is forgotten from the budget. Returns the
    /// number of candidates removed.
    pub fn retain_slots<F>(&self, mut keep: F) -> usize
    where
        F: FnMut(&K) -> bool,
    {
        // Read guard across the slot retention AND the per-candidate
        // budget `forget_seq` — consistent with `admit` / `clear`.
        let _retention = self.retention_gate.read();
        let mut removed = 0usize;
        self.slots.retain(|key, slot| {
            if keep(key) {
                true
            } else {
                let candidates = slot.candidates.lock();
                for c in candidates.iter() {
                    self.budget.forget_seq(c.seq);
                }
                removed += candidates.len();
                false
            }
        });
        removed
    }
}

/// RAII guard returned by
/// [`BoundedCandidateMap::test_arm_clear_midpoint_gate`]. Disarms the
/// per-instance `clear` injection point on drop so a later `clear` on
/// the same map does not park on a stale barrier.
#[cfg(test)]
#[doc(hidden)]
pub struct ClearMidpointGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for ClearMidpointGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
    }
}

/// RAII guard returned by
/// [`BoundedCandidateMap::test_arm_admit_post_record_gate`]. Disarms the
/// per-instance `admit` injection point on drop.
#[cfg(test)]
#[doc(hidden)]
pub struct AdmitPostRecordGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for AdmitPostRecordGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
    }
}

/// RAII guard returned by
/// [`BoundedCandidateMap::test_arm_admit_pre_budget_gate`]. Disarms the
/// per-instance pre-budget `admit` injection point on drop.
#[cfg(test)]
#[doc(hidden)]
pub struct AdmitPreBudgetGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for AdmitPreBudgetGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
    }
}

#[cfg(test)]
#[path = "bounded_query_retention_tests.rs"]
mod tests;
