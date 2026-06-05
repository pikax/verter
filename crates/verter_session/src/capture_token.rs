//! Per-request capture-token harness for test-only counter assertions.
//!
//! Workspace-wide thread-local globals (`thread_local!{ static COUNT: Cell<usize>; }`)
//! are forbidden — they break under `cargo test` parallelism and conflate
//! independent test runs. Every counter assertion reads its value via a
//! `CaptureSnapshot` produced by [`CaptureGuard::end`] against a token
//! bound at [`CaptureToken::start_for_query`].
//!
//! Production-side hooks at the parse-completion site, `record_origin_edge`,
//! `intern_signature`, dispatch entry, and cache hit/miss paths reach the
//! active token via [`with_active_capture`]. When no token is bound (the
//! production hot path), the call returns immediately without touching
//! the thread-local's interior cell — the zero-overhead path.
//!
//! This module is `pub(crate)`; integration tests reach the API through
//! the [`for_tests`](crate::for_tests) re-export module gated
//! `cfg(any(test, debug_assertions))`. Production release builds do not
//! extend the public surface — `for_tests` is absent in the release
//! build configuration.
//!
//! # Why this is not `cfg(test)`-gated
//!
//! Integration tests in `crates/verter_session/tests/*.rs` build as a
//! separate crate target. Cargo compiles the lib for those tests with
//! `cfg(test)` UNSET but `debug_assertions` SET, so the whole module is
//! gated `cfg(any(test, debug_assertions))` (declared in `lib.rs`):
//! 1. `pub(crate)` keeps the API out of the public crate surface.
//! 2. The same gate covers the `for_tests` re-export, so release-build
//!    downstream consumers cannot access it.
//! 3. `cargo build --release` has `debug_assertions` OFF, so neither this
//!    module nor its recording hooks are compiled — release pays zero
//!    cost. Every production `with_active_capture(..)` recording site
//!    carries the matching `#[cfg(any(test, debug_assertions))]` so the
//!    hook simply does not exist in release (no thread-local lookup).

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::semantic_query::{OriginEdgeKind, OriginMeta, SemanticNodeId, SemanticQueryKey};

// ---------------------------------------------------------------------------
// Identity tuples and hashes
// ---------------------------------------------------------------------------

/// Hash of a builder's `DepSignature` snapshot, used as a discriminator on
/// origin edges. The harness does not depend on the structural shape of
/// `DepSignature`; it only needs equality of two derivations.
pub type SignatureHash = u64;

/// Identifier for an interned signature. Mirrors the production interner's
/// id space without binding to the production allocator. `0` is reserved.
pub type InternedId = u64;

/// Canonical id of a parsed file. The parse-count snapshot is keyed by
/// the canonical id string passed to `execute_source`.
pub type CanonicalId = Arc<str>;

/// Identity tuple for an origin edge — used to detect duplicate-emission
/// of the same derivation tuple within a request.
///
/// Two derivations of the same `(result_node, kind, sources)` tuple but with
/// different `dep_signature` or `metadata_hash` are LEGITIMATE and NOT
/// counted as duplicates: `DerivationStore` stores them as parallel edges
/// because they represent the same result derived through different
/// dep contexts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EdgeIdentity {
    pub result_node: SemanticNodeId,
    pub kind: OriginEdgeKind,
    /// Sources are normalized via sort+dedup before insertion so the order
    /// of arms passed to `record_origin_edge` is not a discriminator.
    pub sources: SmallVec<[SemanticNodeId; 4]>,
    pub dep_signature: SignatureHash,
    pub metadata_hash: u64,
}

impl EdgeIdentity {
    /// Build an `EdgeIdentity` from raw record-time inputs. Sources are
    /// sorted and deduplicated so order/duplicates inside the input array
    /// do not affect identity equality.
    #[must_use]
    pub fn new(
        result_node: SemanticNodeId,
        kind: OriginEdgeKind,
        sources: impl IntoIterator<Item = SemanticNodeId>,
        dep_signature: SignatureHash,
        metadata_hash: u64,
    ) -> Self {
        let mut sources: SmallVec<[SemanticNodeId; 4]> = sources.into_iter().collect();
        sources.sort_by_key(|n| n.0);
        sources.dedup_by_key(|n| n.0);
        Self {
            result_node,
            kind,
            sources,
            dep_signature,
            metadata_hash,
        }
    }

    /// Build an `EdgeIdentity` from a production `record_origin_edge`
    /// call's arguments. Hashes the per-edge `OriginMeta` and the
    /// builder fence into `metadata_hash` and `dep_signature` so the
    /// harness does not depend on the internal interner identity.
    #[must_use]
    pub fn from_record(
        result_node: SemanticNodeId,
        kind: OriginEdgeKind,
        sources: &[SemanticNodeId],
        meta: &OriginMeta,
        dep_signature_hash: SignatureHash,
    ) -> Self {
        let metadata_hash = hash_origin_meta(meta);
        Self::new(
            result_node,
            kind,
            sources.iter().copied(),
            dep_signature_hash,
            metadata_hash,
        )
    }
}

fn hash_index_key<H: std::hash::Hasher>(key: &crate::semantic_query::IndexKey, h: &mut H) {
    use crate::semantic_query::IndexKey;
    use std::hash::Hash;
    match key {
        IndexKey::String(name) => {
            0u8.hash(h);
            name.as_ref().hash(h);
        }
        IndexKey::Number(value) => {
            1u8.hash(h);
            value.hash(h);
        }
        IndexKey::TypeNode(id) => {
            2u8.hash(h);
            id.0.hash(h);
        }
    }
}

/// Stable structural hash of an [`OriginMeta`]. `OriginMeta` does not
/// implement `Hash` directly; this helper performs a
/// discriminator-by-discriminator manual walk so the harness can produce
/// a deterministic 64-bit identity for the per-edge `metadata_hash`
/// field.
fn hash_origin_meta(meta: &OriginMeta) -> u64 {
    use crate::semantic_query::PathSegment;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut h = PassThroughHasher(&mut hasher);
    use std::hash::{Hash, Hasher};
    match meta {
        OriginMeta::None => 0u8.hash(&mut h),
        OriginMeta::Branch(b) => {
            1u8.hash(&mut h);
            b.hash(&mut h);
        }
        OriginMeta::ProjectedMember { name, provenance } => {
            2u8.hash(&mut h);
            name.as_ref().hash(&mut h);
            (*provenance as u8).hash(&mut h);
        }
        OriginMeta::AliasName(name) => {
            6u8.hash(&mut h);
            name.as_ref().hash(&mut h);
        }
        OriginMeta::Index(index) => {
            3u8.hash(&mut h);
            hash_index_key(index, &mut h);
        }
        OriginMeta::Path(segments) => {
            4u8.hash(&mut h);
            for segment in segments.iter() {
                match segment {
                    PathSegment::Member(name) => {
                        0u8.hash(&mut h);
                        name.as_ref().hash(&mut h);
                    }
                    PathSegment::Index(index) => {
                        1u8.hash(&mut h);
                        hash_index_key(index, &mut h);
                    }
                }
            }
        }
        OriginMeta::SubstitutedParam(name) => {
            5u8.hash(&mut h);
            name.as_ref().hash(&mut h);
        }
    }
    h.finish()
}

/// One observed dispatch attempt. Records the key shape and whether the
/// dispatcher hit or missed the cache so per-key-family hit/miss counters
/// can be derived from the snapshot.
#[derive(Debug, Clone)]
pub struct DispatchEntry {
    pub key: SemanticQueryKey,
    pub hit: bool,
}

/// Cache identifier for `cache_hits` / `cache_misses` accounting. Phases
/// that opt in to per-cache provenance tagging push entries against one
/// of these enum values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheId {
    /// Sentinel value for cache observations that don't yet carry a tag.
    Unspecified,
}

/// Accumulated per-cache hit/miss counts captured by phases that wire
/// the optional `cache_hits` / `cache_misses` snapshot accessors. Phases
/// that don't tag their reads leave these maps empty.
#[derive(Debug, Default, Clone)]
pub struct CacheProvenance {
    pub hits: FxHashMap<CacheId, u64>,
    pub misses: FxHashMap<CacheId, u64>,
}

/// Filter trait for cache key matching inside `cache_hits` / `cache_misses`.
/// Phases that wire per-cache observations supply a concrete filter
/// implementation; this trait keeps the snapshot API agnostic of the
/// concrete key type.
pub trait CacheKeyFilter {
    fn matches(&self, cache: CacheId) -> bool;
}

// ---------------------------------------------------------------------------
// KeyFamily — name-resolved scoping for dispatch counter assertions
// ---------------------------------------------------------------------------

/// Scoping family for `dispatch_count` / `dispatch_misses` assertions.
///
/// Tests use `KeyFamily::InstantiateForResolvedName("UIMessage")` —
/// the harness compares the family against an observed `SemanticQueryKey`
/// and decides if it matches. The harness operates on the key's surface
/// fields (declaration name, scope canonical, projection mode) so the
/// caller never touches a runtime `DeclId`.
#[derive(Debug, Clone)]
pub enum KeyFamily {
    /// Match `Instantiate { base.merged_symbol_name == name, .. }`
    /// regardless of projection mode and arguments.
    InstantiateForResolvedName(&'static str),
    /// Match `ProjectPath { mode == Navigate }` whose terminal segment
    /// matches the listed alias hops. Empty hops match any `Navigate`
    /// projection rooted at any base.
    NavigateForAlias(&'static str, Vec<&'static str>),
    /// Match `Instantiate { base.merged_symbol_name == name,
    /// context.projection_reduction.mode == Shallow }`.
    ShallowForResolvedName(&'static str),
    /// Match `Instantiate { base.merged_symbol_name == name,
    /// context.projection_reduction.mode == Skeleton }`.
    SkeletonForResolvedName(&'static str),
    /// Match `Instantiate { base.merged_symbol_name == name,
    /// context.projection_reduction.mode == Expanded }`.
    ///
    /// Used by the field-level fast path counterfixtures
    /// to assert that, for a fast-path-eligible field, the macro
    /// shell is NOT dispatched in `Expanded` mode (the cold-time
    /// regression that the fast path eliminates is the Expanded-mode
    /// `Instantiate` whose `base.merged_symbol_name == UIMessage`
    /// dispatch driven by `defineProps<ChatMessageProps extends
    /// UIMessage<...>>()` carriers — counter == 0 after the fast
    /// path takes the field through `exact_concrete(parsed)`).
    InstantiateExpandedForResolvedName(&'static str),
    /// Match `ResolveMacroPayload { macro_kind == DefineSlots }`.
    SlotBindingDispatch,
    /// Always match (used for total-dispatch counters).
    AnyDispatch,
}

impl KeyFamily {
    /// Returns true iff `key` belongs to this family.
    #[must_use]
    pub fn matches(&self, key: &SemanticQueryKey) -> bool {
        use crate::semantic_query::PathSegment;

        match (self, key) {
            (KeyFamily::AnyDispatch, _) => true,
            (
                KeyFamily::InstantiateForResolvedName(name),
                SemanticQueryKey::Instantiate { base, .. },
            ) => base.merged_symbol_name.as_ref() == *name,
            (
                KeyFamily::ShallowForResolvedName(name),
                SemanticQueryKey::Instantiate { base, context, .. },
            ) => {
                base.merged_symbol_name.as_ref() == *name
                    && matches!(
                        context.projection_reduction.mode,
                        crate::semantic_query::ProjectionMode::Shallow
                    )
            }
            (
                KeyFamily::SkeletonForResolvedName(name),
                SemanticQueryKey::Instantiate { base, context, .. },
            ) => {
                base.merged_symbol_name.as_ref() == *name
                    && matches!(
                        context.projection_reduction.mode,
                        crate::semantic_query::ProjectionMode::Skeleton
                    )
            }
            (
                KeyFamily::InstantiateExpandedForResolvedName(name),
                SemanticQueryKey::Instantiate { base, context, .. },
            ) => {
                base.merged_symbol_name.as_ref() == *name
                    && matches!(
                        context.projection_reduction.mode,
                        crate::semantic_query::ProjectionMode::Expanded
                    )
            }
            (
                KeyFamily::NavigateForAlias(_root_name, hops),
                SemanticQueryKey::ProjectPath { path, context, .. },
            ) => {
                if !matches!(
                    context.mode,
                    crate::semantic_query::ProjectionMode::Navigate
                ) {
                    return false;
                }
                if hops.is_empty() {
                    return true;
                }
                if path.len() != hops.len() {
                    return false;
                }
                path.iter()
                    .zip(hops.iter())
                    .all(|(seg, expected)| match seg {
                        PathSegment::Member(member) => member.as_ref() == *expected,
                        PathSegment::Index(_) => false,
                    })
            }
            (
                KeyFamily::SlotBindingDispatch,
                SemanticQueryKey::ResolveMacroPayload { macro_kind, .. },
            ) => matches!(
                macro_kind,
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
            ),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// CaptureToken / Guard / Snapshot
// ---------------------------------------------------------------------------

/// Atomic generation tag — every distinct token gets a unique id so the
/// harness can detect leaked / nested binds. Counter starts at 1 because
/// `0` is reserved as the sentinel "no token" value.
fn next_request_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Per-request capture state — owned by an `Arc` shared between the
/// guard and the thread-local binding so the guard's drop can rebind
/// the cell back to `None` even if production code held a clone.
pub struct CaptureToken {
    /// Identity of this token's request. Read-only after construction.
    pub request_id: u64,
    /// Wall-clock start for `elapsed`.
    started_at: Instant,
    /// Optional human label for the bound query (debug only).
    label: &'static str,
    counters: Mutex<HashMap<&'static str, u64>>,
    parse_count: Mutex<HashMap<CanonicalId, u32>>,
    edge_ledger: Mutex<HashSet<EdgeIdentity>>,
    /// Count of duplicate observations — incremented on every
    /// `record_edge` call whose identity tuple is already present.
    duplicate_edges: Mutex<u64>,
    intern_ledger: Mutex<HashMap<SignatureHash, InternedId>>,
    /// Count of `record_intern` calls that returned an existing id.
    intern_returned_existing: Mutex<u64>,
    /// Count of `record_intern` calls that allocated a new id.
    intern_returned_new: Mutex<u64>,
    cache_provenance: Mutex<CacheProvenance>,
    dispatch_log: Mutex<Vec<DispatchEntry>>,
    // -------------------------------------------------------------------
    // diagnosis counters
    //
    // These counters profile cost contributors to the
    // `repo_first_pass` semantic-state regression. They are read
    // directly off the snapshot via the corresponding accessors;
    // delta-accumulated under capture only (no workspace-wide
    // statics). Hooks are no-ops when no token is bound.
    // -------------------------------------------------------------------
    /// Total wall-clock time (ns) spent inside `record_origin_edge`.
    record_origin_edge_total_ns: Mutex<u128>,
    /// Number of `record_origin_edge` invocations.
    origin_edge_count: Mutex<u64>,
    /// Snapshot of the derivation-signature pool size at capture-end.
    derivation_signature_pool_size: Mutex<u64>,
    /// Number of `intern_signature` invocations.
    derivation_signature_intern_calls: Mutex<u64>,
    /// Number of `intern_signature` invocations that returned an
    /// already-interned `Arc<DepSignature>` (no allocation).
    derivation_signature_intern_returned_existing: Mutex<u64>,
    /// Total wall-clock time (ns) threads waited to acquire the entries
    /// mutex on the `SemanticGraphStore`.
    entries_mutex_wait_total_ns: Mutex<u128>,
    /// Total wall-clock time (ns) threads held the entries mutex.
    entries_mutex_hold_total_ns: Mutex<u128>,
}

impl CaptureToken {
    /// Bind a fresh capture token to the current thread for the duration
    /// of the returned guard. Panics if a token is already bound on this
    /// thread (Invariant: misuse-loud).
    ///
    /// The guard's drop runs `try_unbind` so a forgotten `end()` does not
    /// poison the thread-local — but the snapshot is only available
    /// through `end()`.
    #[must_use]
    pub fn start_for_query(label: &'static str) -> CaptureGuard {
        ACTIVE.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_some() {
                panic!("nested CaptureToken::start_for_query (existing token bound)");
            }
            let token = Arc::new(CaptureToken {
                request_id: next_request_id(),
                started_at: Instant::now(),
                label,
                counters: Mutex::new(HashMap::new()),
                parse_count: Mutex::new(HashMap::new()),
                edge_ledger: Mutex::new(HashSet::new()),
                duplicate_edges: Mutex::new(0),
                intern_ledger: Mutex::new(HashMap::new()),
                intern_returned_existing: Mutex::new(0),
                intern_returned_new: Mutex::new(0),
                cache_provenance: Mutex::new(CacheProvenance::default()),
                dispatch_log: Mutex::new(Vec::new()),
                record_origin_edge_total_ns: Mutex::new(0),
                origin_edge_count: Mutex::new(0),
                derivation_signature_pool_size: Mutex::new(0),
                derivation_signature_intern_calls: Mutex::new(0),
                derivation_signature_intern_returned_existing: Mutex::new(0),
                entries_mutex_wait_total_ns: Mutex::new(0),
                entries_mutex_hold_total_ns: Mutex::new(0),
            });
            *slot = Some(Arc::clone(&token));
            CaptureGuard { token }
        })
    }

    /// Increment a named counter by `delta`.
    pub fn record_counter(&self, name: &'static str, delta: u64) {
        let mut map = self.counters.lock();
        let entry = map.entry(name).or_insert(0);
        *entry = entry.saturating_add(delta);
    }

    /// Record one parse-completion event for `canonical`.
    pub fn record_parse(&self, canonical: &str) {
        let mut map = self.parse_count.lock();
        let entry = map.entry(Arc::<str>::from(canonical)).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// Record a derivation/origin edge by its identity tuple. Increments
    /// the duplicate counter when the tuple is already present.
    pub fn record_edge(&self, identity: EdgeIdentity) {
        let mut ledger = self.edge_ledger.lock();
        if !ledger.insert(identity) {
            // Identity already present — count as duplicate.
            let mut dup = self.duplicate_edges.lock();
            *dup = dup.saturating_add(1);
        }
    }

    /// Record one signature-intern observation. Returns `(returned_existing, id)`
    /// where `returned_existing` is true when `signature` was already in
    /// the ledger.
    pub fn record_intern(&self, signature: SignatureHash, id: InternedId) -> bool {
        let mut ledger = self.intern_ledger.lock();
        if let Some(existing) = ledger.get(&signature) {
            let already_existing = *existing == id;
            // Only count as "returned_existing" when the id matches; a
            // mismatch is a real new allocation that happened to have a
            // colliding key, which would indicate a cache bug.
            let mut count = self.intern_returned_existing.lock();
            *count = count.saturating_add(1);
            return already_existing;
        }
        ledger.insert(signature, id);
        let mut count = self.intern_returned_new.lock();
        *count = count.saturating_add(1);
        false
    }

    /// Record one observed dispatch with its hit/miss outcome.
    pub fn record_dispatch(&self, key: &SemanticQueryKey, hit: bool) {
        let mut log = self.dispatch_log.lock();
        log.push(DispatchEntry {
            key: key.clone(),
            hit,
        });
    }

    /// Record a cache hit against `db`.
    pub fn record_cache_hit(&self, db: CacheId) {
        let mut prov = self.cache_provenance.lock();
        let entry = prov.hits.entry(db).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// Record a cache miss against `db`.
    pub fn record_cache_miss(&self, db: CacheId) {
        let mut prov = self.cache_provenance.lock();
        let entry = prov.misses.entry(db).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    // ---------------------------------------------------------------
    // diagnosis recorders
    // ---------------------------------------------------------------

    /// Record one `record_origin_edge` invocation with its measured
    /// wall-clock cost in nanoseconds. Bumps the call counter and the
    /// total-cost accumulator under capture; no-op outside capture.
    pub fn record_origin_edge_call(&self, elapsed_ns: u128) {
        let mut total = self.record_origin_edge_total_ns.lock();
        *total = total.saturating_add(elapsed_ns);
        let mut count = self.origin_edge_count.lock();
        *count = count.saturating_add(1);
    }

    /// Record one signature-interning invocation. `returned_existing`
    /// is true when the lookup returned an already-interned
    /// `Arc<DepSignature>` (no allocation).
    pub fn record_signature_intern(&self, returned_existing: bool) {
        let mut calls = self.derivation_signature_intern_calls.lock();
        *calls = calls.saturating_add(1);
        if returned_existing {
            let mut hits = self.derivation_signature_intern_returned_existing.lock();
            *hits = hits.saturating_add(1);
        }
    }

    /// Record the size of the derivation-signature pool at the moment
    /// the capture closes. Called at most once per capture; later
    /// writes overwrite the prior snapshot.
    pub fn record_signature_pool_size(&self, size: u64) {
        *self.derivation_signature_pool_size.lock() = size;
    }

    /// Record a single entries-mutex acquisition: wait time + hold time
    /// in nanoseconds. Both deltas accumulate; a zero value for either
    /// is acceptable (uncontended fast path).
    pub fn record_entries_mutex_timing(&self, wait_ns: u128, hold_ns: u128) {
        let mut total_wait = self.entries_mutex_wait_total_ns.lock();
        *total_wait = total_wait.saturating_add(wait_ns);
        let mut total_hold = self.entries_mutex_hold_total_ns.lock();
        *total_hold = total_hold.saturating_add(hold_ns);
    }
}

/// RAII guard for a bound capture token. Drop unbinds the thread-local
/// without producing a snapshot; consumers that need the snapshot must
/// call [`end`](CaptureGuard::end) explicitly.
pub struct CaptureGuard {
    token: Arc<CaptureToken>,
}

impl CaptureGuard {
    /// Consume the guard and return the immutable snapshot.
    #[must_use]
    pub fn end(self) -> CaptureSnapshot {
        let token = Arc::clone(&self.token);
        // Drop releases the thread-local binding. Once we extract the
        // snapshot data we drop the Arc reference held by the guard.
        drop(self);
        let elapsed = token.started_at.elapsed();
        let counters = token.counters.lock().clone();
        let parse_count = token.parse_count.lock().clone();
        let edge_ledger = token.edge_ledger.lock().clone();
        let duplicate_edges = *token.duplicate_edges.lock();
        let intern_ledger = token.intern_ledger.lock().clone();
        let intern_returned_existing = *token.intern_returned_existing.lock();
        let intern_returned_new = *token.intern_returned_new.lock();
        let cache_provenance = token.cache_provenance.lock().clone();
        let dispatch_log = token.dispatch_log.lock().clone();
        let record_origin_edge_total_ns = *token.record_origin_edge_total_ns.lock();
        let origin_edge_count = *token.origin_edge_count.lock();
        let derivation_signature_pool_size = *token.derivation_signature_pool_size.lock();
        let derivation_signature_intern_calls = *token.derivation_signature_intern_calls.lock();
        let derivation_signature_intern_returned_existing =
            *token.derivation_signature_intern_returned_existing.lock();
        let entries_mutex_wait_total_ns = *token.entries_mutex_wait_total_ns.lock();
        let entries_mutex_hold_total_ns = *token.entries_mutex_hold_total_ns.lock();
        CaptureSnapshot {
            request_id: token.request_id,
            label: token.label,
            elapsed,
            counters,
            parse_count,
            edge_ledger,
            duplicate_edges,
            intern_ledger,
            intern_returned_existing,
            intern_returned_new,
            cache_provenance,
            dispatch_log,
            record_origin_edge_total_ns,
            origin_edge_count,
            derivation_signature_pool_size,
            derivation_signature_intern_calls,
            derivation_signature_intern_returned_existing,
            entries_mutex_wait_total_ns,
            entries_mutex_hold_total_ns,
        }
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        ACTIVE.with(|cell| {
            let mut slot = cell.borrow_mut();
            // If the slot still points at our token, clear it. If a
            // different token is bound, leave it alone — the panic in
            // `start_for_query` already prevents nested binds, so this
            // branch is only taken when the test deliberately rebinds.
            if let Some(active) = slot.as_ref() {
                if Arc::ptr_eq(active, &self.token) {
                    *slot = None;
                }
            }
        });
    }
}

/// Immutable per-request snapshot of all counters captured between
/// `start_for_query` and `end`. Tests assert against the snapshot's
/// inspector methods rather than touching internal state.
#[derive(Debug, Clone)]
pub struct CaptureSnapshot {
    pub request_id: u64,
    pub label: &'static str,
    pub elapsed: Duration,
    pub counters: HashMap<&'static str, u64>,
    pub parse_count: HashMap<CanonicalId, u32>,
    pub edge_ledger: HashSet<EdgeIdentity>,
    pub duplicate_edges: u64,
    pub intern_ledger: HashMap<SignatureHash, InternedId>,
    pub intern_returned_existing: u64,
    pub intern_returned_new: u64,
    pub cache_provenance: CacheProvenance,
    pub dispatch_log: Vec<DispatchEntry>,
    // -------------------------------------------------------------------
    // diagnosis counter snapshots
    // -------------------------------------------------------------------
    /// Total wall-clock cost (ns) of all `record_origin_edge` calls
    /// during this capture window.
    pub record_origin_edge_total_ns: u128,
    /// Number of `record_origin_edge` calls during this capture window.
    pub origin_edge_count: u64,
    /// Size of the derivation-signature pool at capture-end (a
    /// snapshot rather than a delta — the pool is process-wide, but
    /// recorded by the producer at end-of-capture).
    pub derivation_signature_pool_size: u64,
    /// Number of `intern_signature` invocations during this capture.
    pub derivation_signature_intern_calls: u64,
    /// Number of `intern_signature` invocations that returned an
    /// already-interned `Arc` (no fresh allocation).
    pub derivation_signature_intern_returned_existing: u64,
    /// Total wall-clock time (ns) threads waited to acquire the
    /// `SemanticGraphStore::entries` mutex during this capture.
    pub entries_mutex_wait_total_ns: u128,
    /// Total wall-clock time (ns) threads held the
    /// `SemanticGraphStore::entries` mutex during this capture.
    pub entries_mutex_hold_total_ns: u128,
}

impl CaptureSnapshot {
    /// Read named counter, returning 0 when absent.
    #[must_use]
    pub fn counter(&self, name: &'static str) -> u64 {
        self.counters.get(name).copied().unwrap_or(0)
    }

    /// Total number of dispatch observations whose key matches `family`.
    #[must_use]
    pub fn dispatch_count(&self, family: KeyFamily) -> u64 {
        self.dispatch_log
            .iter()
            .filter(|entry| family.matches(&entry.key))
            .count() as u64
    }

    /// Number of dispatch observations whose key matches `family` AND
    /// missed the cache.
    #[must_use]
    pub fn dispatch_misses(&self, family: KeyFamily) -> u64 {
        self.dispatch_log
            .iter()
            .filter(|entry| family.matches(&entry.key) && !entry.hit)
            .count() as u64
    }

    /// Number of times `canonical` was parsed under the active token.
    #[must_use]
    pub fn parse_count_for(&self, canonical: &str) -> u32 {
        self.parse_count
            .iter()
            .find(|(k, _)| k.as_ref() == canonical)
            .map(|(_, v)| *v)
            .unwrap_or(0)
    }

    /// Number of cache observations against `db` that hit, filtered by
    /// `key` (filter has access to the cache id; phases supply
    /// concrete filters).
    #[must_use]
    pub fn cache_hits(&self, db: CacheId, _key: &dyn CacheKeyFilter) -> u64 {
        self.cache_provenance.hits.get(&db).copied().unwrap_or(0)
    }

    /// Number of cache observations against `db` that missed.
    #[must_use]
    pub fn cache_misses(&self, db: CacheId, _key: &dyn CacheKeyFilter) -> u64 {
        self.cache_provenance.misses.get(&db).copied().unwrap_or(0)
    }

    /// Number of distinct edge-identity tuples observed.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edge_ledger.len()
    }

    /// Number of duplicate edge observations (per §16.3 "exact identity
    /// tuple" — all five fields equal).
    #[must_use]
    pub fn duplicate_edge_count(&self) -> usize {
        self.duplicate_edges as usize
    }

    /// Number of `record_intern` calls that returned an already-present id.
    #[must_use]
    pub fn intern_returned_existing_count(&self) -> usize {
        self.intern_returned_existing as usize
    }

    /// Number of `record_intern` calls that allocated a fresh id.
    #[must_use]
    pub fn intern_returned_new_count(&self) -> usize {
        self.intern_returned_new as usize
    }
}

// ---------------------------------------------------------------------------
// Thread-local binding + production-side hook
// ---------------------------------------------------------------------------

thread_local! {
    static ACTIVE: std::cell::RefCell<Option<Arc<CaptureToken>>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `f` against the currently-bound token, if any.
///
/// This is the only entry point production hooks use to record events.
/// When no token is bound the function returns immediately without
/// running `f` and without lock acquisition — the zero-overhead path.
///
/// Closures may return `()` only; if a caller needs the return value
/// they should call [`with_active_capture_returning`] which is generic
/// over the closure's return type.
pub fn with_active_capture(f: impl for<'a> FnOnce(&'a CaptureToken)) {
    ACTIVE.with(|cell| {
        // `try_borrow` so a re-entrant capture call inside `f` cannot
        // deadlock the RefCell. The active token is never re-entered
        // from inside its own callback in production, but a defensive
        // guard keeps the harness loud rather than silent if the
        // contract is violated.
        let token = match cell.try_borrow().ok().and_then(|slot| slot.clone()) {
            Some(t) => t,
            None => return,
        };
        f(&token);
    });
}

/// Generic-return variant of [`with_active_capture`]. Returns `None`
/// when no token is bound on the current thread; otherwise returns the
/// closure's result wrapped in `Some`. Used by tests that observe
/// whether a token was bound.
pub fn with_active_capture_returning<R>(
    f: impl for<'a> FnOnce(&'a CaptureToken) -> R,
) -> Option<R> {
    ACTIVE.with(|cell| {
        let token = cell.try_borrow().ok().and_then(|slot| slot.clone())?;
        Some(f(&token))
    })
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

/// Stable hash of a slice of `Hash`-able items, used by production
/// hooks that need to summarise an `Arc<[T]>` payload (such as a
/// builder dep-signature) into a `SignatureHash`.
pub(crate) fn stable_hash_slice<T: Hash>(slice: &[T]) -> u64 {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = PassThroughHasher(&mut hasher);
    slice.hash(&mut bridge);
    hasher.digest()
}

/// Bridge between `std::hash::Hash` and `xxhash_rust::xxh3::Xxh3`.
struct PassThroughHasher<'a>(&'a mut xxhash_rust::xxh3::Xxh3);

impl<'a> std::hash::Hasher for PassThroughHasher<'a> {
    fn finish(&self) -> u64 {
        self.0.digest()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

// ---------------------------------------------------------------------------
// Stack-overflow detection helper
// ---------------------------------------------------------------------------

/// Result of a stack-overflow-checked invocation.
#[derive(Debug)]
pub struct StackOverflow;

impl std::fmt::Display for StackOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stack overflow detected on bounded-stack thread")
    }
}

impl std::error::Error for StackOverflow {}

/// Run `f` on a thread with a 384 KiB stack. Returns `Err(StackOverflow)`
/// when the closure panics with a payload that names "stack overflow"
/// (or its Windows-style `STATUS_STACK_OVERFLOW` form). Used by
/// (cycle guard) tests to assert recursive codepaths terminate before
/// running out of stack — when the guard is wrongly keyed, the closure
/// will recurse unboundedly and the small stack hits OS-level overflow
/// quickly. The cap is small enough that an unbounded recursion still
/// overflows fast, yet generous enough that a legitimate deep
/// resolution (which nests one cold build per hop) does not false-trip.
///
/// # Platform behavior
///
/// On **Linux/macOS**, an OS stack overflow on a child thread is
/// recoverable: the runtime converts the SIGSEGV at the guard page into
/// a thread panic that surfaces through `JoinHandle::join` as `Err`.
/// This function returns `Err(StackOverflow)` in that case.
///
/// On **Windows**, an OS stack overflow on a child thread **aborts the
/// entire process** with `STATUS_STACK_OVERFLOW` (exit code `0xC0000FD`).
/// The parent test process dies and the test runner reports a process
/// abort. This is NOT recoverable from Rust and `JoinHandle::join` never
/// returns. Tests that need to assert "the closure overflows" therefore
/// must use a cooperative depth-budget check inside the closure (panic
/// explicitly with a message containing "stack overflow") rather than
/// relying on the OS guard page.
///
/// Tests that need to assert "the closure does NOT overflow" run as
/// normal: if the closure completes the function returns `Ok(value)`;
/// if it overflows the test process aborts, which the runner reports.
pub fn assert_no_stack_overflow<F, R>(f: F) -> Result<R, StackOverflow>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // A small stack so an UNBOUNDED recursion in `f` overflows fast.
    // The cap accounts for the per-frame cost of the cold-build
    // cooperative-admission path — the strict warm-read validator
    // threads a resolver-context handle through every nested cold
    // build of a deep type resolution.
    let builder = thread::Builder::new()
        .name("assert_no_stack_overflow".into())
        .stack_size(384 * 1024);
    let handle = builder
        .spawn(move || {
            // Wrap in `catch_unwind` so an explicit panic inside `f`
            // surfaces as `Err(payload)` to the join logic, while a
            // stack-overflow abort takes the thread down without
            // unwinding.
            panic::catch_unwind(AssertUnwindSafe(f))
        })
        .expect("spawn bounded-stack thread");
    match handle.join() {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(panic_payload)) => {
            // The closure panicked. Inspect the payload for a
            // "stack overflow" marker; if present, classify as
            // StackOverflow. Otherwise propagate the panic.
            let message = panic_payload_to_string(&panic_payload);
            if is_stack_overflow_marker(&message) {
                Err(StackOverflow)
            } else {
                std::panic::resume_unwind(panic_payload);
            }
        }
        Err(_join_err) => {
            // Thread aborted — only reachable on platforms where the
            // OS converts stack overflow to a thread-only abort
            // (Linux/macOS).
            Err(StackOverflow)
        }
    }
}

fn panic_payload_to_string(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    String::new()
}

fn is_stack_overflow_marker(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("stack overflow") || lower.contains("status_stack_overflow")
}

// ---------------------------------------------------------------------------
// Tests for the harness itself
// ---------------------------------------------------------------------------

#[cfg(test)]
mod self_tests {
    use super::*;

    #[test]
    fn record_counter_under_active_token_is_visible_in_snapshot() {
        let guard = CaptureToken::start_for_query("self_test_counter");
        with_active_capture(|t| t.record_counter("a", 4));
        with_active_capture(|t| t.record_counter("a", 1));
        let snap = guard.end();
        assert_eq!(snap.counter("a"), 5);
    }

    #[test]
    fn no_active_token_means_no_op() {
        // Without `start_for_query` the call must not panic and must
        // return None (no closure invocation).
        let invoked = with_active_capture_returning(|_t| 42usize);
        assert!(invoked.is_none());
    }

    #[test]
    fn diagnosis_counters_round_trip_through_snapshot() {
        let guard = CaptureToken::start_for_query("self_test_diagnosis");
        with_active_capture(|t| t.record_origin_edge_call(1_500));
        with_active_capture(|t| t.record_origin_edge_call(2_500));
        with_active_capture(|t| t.record_signature_intern(false));
        with_active_capture(|t| t.record_signature_intern(true));
        with_active_capture(|t| t.record_signature_intern(true));
        with_active_capture(|t| t.record_signature_pool_size(42));
        with_active_capture(|t| t.record_entries_mutex_timing(100, 800));
        with_active_capture(|t| t.record_entries_mutex_timing(50, 200));
        let snap = guard.end();
        assert_eq!(snap.origin_edge_count, 2);
        assert_eq!(snap.record_origin_edge_total_ns, 4_000);
        assert_eq!(snap.derivation_signature_intern_calls, 3);
        assert_eq!(snap.derivation_signature_intern_returned_existing, 2);
        assert_eq!(snap.derivation_signature_pool_size, 42);
        assert_eq!(snap.entries_mutex_wait_total_ns, 150);
        assert_eq!(snap.entries_mutex_hold_total_ns, 1_000);
    }
}
