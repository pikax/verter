//! Typed compile-output cache nodes.
//!
//! Two typed nodes own the read/write surface for compiled SFC output:
//!
//! * [`CompileOutputNodePureContent`] — a content-addressed
//!   [`ArtifactNode`] keyed by every deterministic input the compiled
//!   bytes depend on. One entry per key; no fact-validation rail. Used
//!   by callers that opt into pure content-keyed reuse and accept that
//!   cross-file edits invalidate only through env-hash bumps.
//!
//! * [`CompileOutputNodeFactValidatedSession`] — a query-identity
//!   [`QueryNode`] keyed by `(canonical_id, profile_hash)`. Multi-
//!   candidate slots coexist under the per-slot cap, each candidate
//!   validated against the path-precise [`ReadSetSignature`]. Backed
//!   by the per-profile [`ProfileState::compile_slots`] table — the
//!   public interface is this node's typed methods; direct
//!   `compile_slots` access from outside this module is forbidden
//!   (`virtual_file_pipeline.rs` routes its reads and writes through
//!   the methods below).
//!
//! Both nodes are stateless adapters over host storage: the
//! [`PureContent`] entries live in this struct's [`DashMap`]; the
//! [`FactValidatedSession`] entries live on the host's
//! [`ProfileState`] map. The node types carry only the inflight
//! tables and (for `PureContent`) the entry map; the host hands the
//! node a `&ProfileState` (or the compile-cache shard) at call time.
//!
//! `#![allow(dead_code)]` at module scope: the routing in
//! `virtual_file_pipeline.rs` consumes the substrate types and methods
//! selectively. The inline `tests` module exercises every public
//! surface independently of the routing.
#![allow(dead_code)]

use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHashMap;

use super::admission::{
    CacheAdmission, CacheEntry, Candidate, DeferredVictims, FactCandidateDiscriminant,
    PublishCoreOutcome, PublishOutcome, SignatureAdmission,
};
use super::node::{ArtifactNode, ComputeCtx, QueryFlightKey, QueryNode};
use super::singleflight::InflightTable;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::types::{
    CachedTsx, CachedVirtualFile, CompileSlot, DiagnosticsSnapshot, Hash16, ProfileState,
    VirtualNodeKind,
};

// ── Key shapes ────────────────────────────────────────────────────────

/// Cache key for the content-addressed compile-output node.
///
/// Every byte-determined input the compiled artifact depends on enters
/// the key: the source canonical and its content hash, the four split
/// env-dimension hashes from [`super::world_snapshot::CompileEnvDims`],
/// the public-API mode hash, the source-map policy hash, and the
/// compiler / plugin version hashes. Two requests that agree on every
/// key dimension MUST produce byte-identical output, so a single
/// content entry serves both — no fact-validation rail is required.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct CompileOutputPureContentKey {
    /// Canonical id of the SFC whose output is cached.
    pub canonical_id: Arc<str>,
    /// `whole_hash` of the source content at compile time.
    pub content_hash: Hash16,
    /// Parse-domain env hash (lib + parser flags).
    pub parse_env_hash: Hash16,
    /// Resolve-domain env hash (workspace aliases, paths, etc.).
    pub resolve_env_hash: Hash16,
    /// Type-domain env hash (lib.d.ts, compilerOptions).
    pub type_env_hash: Hash16,
    /// Library env hash (the global TS lib set).
    pub lib_env_hash: Hash16,
    /// Project identity hash (tsconfig path, project root).
    pub project_identity: Hash16,
    /// Public-API mode hash projecting the compile mode discriminator.
    pub compile_cache_mode_hash: Hash16,
    /// Source-map emission policy hash.
    pub source_map_policy_hash: Hash16,
    /// Compiler crate semantic-version hash.
    pub compiler_version: Hash16,
    /// Plugin set semantic-version hash.
    pub plugin_versions: Hash16,
}

/// Slot key for the fact-validated session compile-output node.
///
/// Per R6, this key carries NO content/version hash — multi-candidate
/// slots coexist on the same `(canonical, profile_hash)` pair, each
/// candidate validated by its own fact signature against the caller's
/// live view. The `profile_hash` is the same `u64` the legacy
/// `compile_slots` table is keyed by; carrying it here keeps the
/// session-node slot key one-to-one with the underlying storage.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct CompileOutputSessionKey {
    /// Canonical id of the SFC whose output is cached.
    pub canonical_id: Arc<str>,
    /// Profile-hash discriminator (same as `ProfileState.compile_slots`
    /// map key).
    pub profile_hash: u64,
}

// ── Value shape ───────────────────────────────────────────────────────

/// Cached compile output. Mirrors the publishable subset of
/// [`CompileSlot`] used by both typed nodes — virtual-file outputs,
/// diagnostics, optional fallback / IDE artifacts, and the captured
/// hashes that the legacy warm-hit pre-filter consults. The fact rail
/// lives separately on the [`ArtifactNode::Value`] / [`Candidate`]
/// envelope, NOT inside the value (so the value is identical between
/// pure-content and session-mode entries).
#[derive(Clone)]
pub(crate) struct CompileOutputValue {
    /// Cached semantic hash of the source content at compile time.
    pub semantic_hash: Hash16,
    /// Cached style-override hash captured at publish.
    pub style_override_hash: u64,
    /// Cached content-override hash captured at publish.
    pub content_override_hash: u64,
    /// Per-virtual-node-kind outputs (Script, Template, Style, Main,
    /// Custom).
    pub outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
    /// Snapshot of compile diagnostics published with this entry.
    pub diagnostics: DiagnosticsSnapshot,
    /// Optional last-good outputs for `DevServeLastKnownGood`.
    pub last_good_outputs: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
    /// Optional combined TSX output (IDE / LSP).
    pub tsx: Option<CachedTsx>,
    /// Optional template-analysis snapshot extracted during compile.
    pub template_analysis: Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
}

impl CompileOutputValue {
    /// Build a value from a compile-tier publish record. Threads the
    /// override + semantic hashes and the per-kind outputs unchanged.
    pub(crate) fn from_compile_record(
        semantic_hash: Hash16,
        style_override_hash: u64,
        content_override_hash: u64,
        outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
        diagnostics: DiagnosticsSnapshot,
        last_good_outputs: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
        tsx: Option<CachedTsx>,
        template_analysis: Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
    ) -> Self {
        Self {
            semantic_hash,
            style_override_hash,
            content_override_hash,
            outputs,
            diagnostics,
            last_good_outputs,
            tsx,
            template_analysis,
        }
    }
}

// ── PureContent node ──────────────────────────────────────────────────

/// Content-addressed compile-output cache node.
///
/// Implements [`ArtifactNode`] over a `DashMap<Key, Arc<CacheEntry>>`.
/// No fact-validation rail — the key already carries every observable
/// env / profile dimension, so two requests with byte-identical keys
/// MUST produce byte-identical output and a single warm entry serves
/// both.
///
/// This node is NOT driven through [`crate::cache_runtime::lookup`].
/// The production callsite (`virtual_file_pipeline.rs`) consults the
/// store with [`Self::peek`] and, on a miss, cold-builds the output
/// inline (the `compile_entry` path) and admits the fresh value through
/// [`Self::publish_content`]. Cold-build deduplication is therefore not
/// a concern of this node:
///
/// * `Session`-mode compile cold-build singleflight is owned by the
///   scheduler (the fact-validated session node coordinates concurrent
///   cold requests).
/// * `Content`-mode cold-build needs no node-level dedup: the key is
///   content-addressed, so a byte-identical key yields a byte-identical
///   value, and a concurrent duplicate cold-build merely recomputes the
///   same bytes and publishes the same entry idempotently.
///
/// The `entries` map is the store; the [`ArtifactNode`] trait also
/// requires an `inflight` table and a `compute` arm, both supplied
/// below. The `compute` arm fails loud (it is never reached on the
/// inline cold-build path) so any future `lookup` consumer cannot
/// silently short-circuit through an unimplemented compute path.
pub(crate) struct CompileOutputNodePureContent {
    entries: DashMap<CompileOutputPureContentKey, Arc<CacheEntry<Arc<CompileOutputValue>>>>,
    /// Per-canonical reverse index: `canonical_id` → the set of content
    /// keys published for that canonical. Maintained alongside `entries`
    /// on every `publish_content` / `remove` / `clear_all` so a targeted
    /// per-file invalidation can evict every content entry for one
    /// canonical without enumerating the full key space.
    ///
    /// This is invalidation plumbing for a content-addressed store, NOT a
    /// fact-validation rail: a key's content / env dimensions remain its
    /// sole identity, and the reverse index never participates in cache
    /// validity. It mirrors the session node's
    /// `clear_compile_outputs_for_file` so explicit
    /// `invalidate_compile_slots` / file-removal callers flush the
    /// content-addressed entries for a canonical the same way they flush
    /// the per-profile session slots.
    by_canonical: DashMap<Arc<str>, rustc_hash::FxHashSet<CompileOutputPureContentKey>>,
    /// Required by the [`ArtifactNode`] trait. Unused on the inline
    /// peek / publish cold-build path this node actually takes — see the
    /// struct doc for why `Content` needs no node-level singleflight.
    inflight: InflightTable<QueryFlightKey<CompileOutputPureContentKey>>,
}

impl CompileOutputNodePureContent {
    /// Construct a fresh node with empty storage.
    pub(crate) fn new() -> Self {
        Self {
            entries: DashMap::new(),
            by_canonical: DashMap::new(),
            inflight: InflightTable::new(),
        }
    }

    /// Read-only peek for a pure-content entry. Returns the cached
    /// value when an entry exists. Validation against the caller's
    /// live world generation is the caller's responsibility — for
    /// pure-content mode, the env-hash dimensions in the key already
    /// invalidate on every observable env change.
    pub(crate) fn peek(
        &self,
        key: &CompileOutputPureContentKey,
    ) -> Option<Arc<CompileOutputValue>> {
        self.entries.get(key).map(|e| e.value.clone())
    }

    /// Publish a freshly compiled value into the content-addressed
    /// store. The value is wrapped in `Arc` at admission so subsequent
    /// peeks pay only the refcount bump.
    pub(crate) fn publish_content(
        &self,
        key: CompileOutputPureContentKey,
        value: CompileOutputValue,
        validated_at_generation: u64,
    ) {
        let entry = CacheEntry {
            value: Arc::new(value),
            signature: ReadSetSignature::new(Arc::from(Vec::new().as_slice())),
            self_root_canonicals: Arc::from(Vec::new().as_slice()),
            validated_at_generation,
        };
        // `entries` is inserted BEFORE `by_canonical` so no `entries`
        // row can ever be orphaned: a concurrent `remove_canonical`
        // interleaved with this publish can only run before
        // `entries.insert` (in which case the new entry will be inserted
        // afterward and the next `remove_canonical` finds it via the
        // backref this publish installs) or after `entries.insert` (in
        // which case the entry is already in `entries` and the racing
        // `remove_canonical` evicts it). The inverse ordering
        // (`by_canonical` first) would orphan an `entries` row whose
        // backref a racing `remove_canonical` already evicted — that
        // orphan would be permanently un-evictable by canonical. This
        // invariant is locked down by the deterministic
        // `publish_orders_entry_before_reverse_index_so_remove_canonical_always_evicts`
        // test and the 20k-cycle concurrent race in
        // `concurrent_publish_and_remove_canonical_never_orphans_content_entry`.
        //
        // The asymmetric tail: the two maps are not lock-coupled, so a
        // `remove_canonical` interleaved BETWEEN `entries.insert` and
        // `by_canonical.insert` splits into two subcases by whether a
        // prior `by_canonical` row already existed for this canonical:
        //
        // - Prior backref existed (an earlier publish for `canonical`
        //   landed and was not yet evicted): `remove_canonical` removes
        //   the prior `by_canonical` row and walks its key set,
        //   `entries.remove`-ing each PRIOR key. Those prior keys are
        //   distinct from this publish's `key` (the racer's `entries`
        //   row at THIS key was just installed and is not in the prior
        //   set), so `entries.remove(this_key)` is never reached. This
        //   publish's `entries` row remains live; its
        //   `by_canonical.insert` then installs a NEW backref pointing
        //   at the now-live `entries` row. RESULT: live backref → live
        //   entry. No orphan, no dust.
        //
        // - No prior backref (first publish for `canonical`, or a prior
        //   remove already drained it): `remove_canonical`'s
        //   `by_canonical.remove(canonical)` returns `None`, the loop
        //   that calls `entries.remove` does not run, and this publish's
        //   `entries` row at `this_key` is untouched. The publish then
        //   installs the first `by_canonical` backref. RESULT: live
        //   backref → live entry. No orphan.
        //
        // - Re-publish at the same key (an identical compile runs twice
        //   between invalidations, so `this_key` IS already in the
        //   prior `by_canonical[canonical]` set): a `remove_canonical`
        //   interleaved between this publish's `entries.insert` and
        //   `by_canonical.insert` will walk the prior key set and
        //   `entries.remove(this_key)` AFTER this publish installed it,
        //   then drop the `by_canonical` row. This publish's later
        //   `by_canonical.insert(this_key)` then re-installs a backref
        //   pointing at no live `entries` row. RESULT: bounded transient
        //   dust — a stale backref with no entry. The dust state is
        //   self-cleaning: the next `remove_canonical` for `canonical`
        //   drains the dust backref via the reverse index (its
        //   `entries.remove(this_key)` is a no-op, which is harmless),
        //   and `clear_all` flushes both maps unconditionally. The bound
        //   is at most one stale backref per concurrent same-key
        //   re-publisher outstanding against this `canonical`. The dust
        //   cannot escape the cache-runtime substrate: the
        //   force-recompute contract still holds because a future
        //   `Content` request after the next `remove_canonical` finds
        //   no `entries` row and recomputes. The
        //   `concurrent_publish_and_remove_canonical_never_orphans_content_entry`
        //   20k-cycle race does NOT exercise this subcase (each cycle
        //   publishes a distinct key, so `this_key` is never already in
        //   the prior set); the bound is established by the reverse-
        //   index drain semantics above rather than by that stress test.
        //
        // The remaining race is a `remove_canonical` that runs AFTER
        // this publish completes both inserts: that case is the
        // standard eviction path — the backref is drained, the entries
        // row is removed, no asymmetric state remains. The
        // `concurrent_publish_and_remove_canonical_never_orphans_content_entry`
        // 20k-cycle race characterises the union of these subcases.
        self.entries.insert(key.clone(), Arc::new(entry));
        self.by_canonical
            .entry(Arc::clone(&key.canonical_id))
            .or_default()
            .insert(key);
    }

    /// Remove the entry for `key`, if any. Keeps the per-canonical
    /// reverse index consistent — the key is dropped from its
    /// canonical's set, and an emptied set is pruned.
    pub(crate) fn remove(&self, key: &CompileOutputPureContentKey) {
        self.entries.remove(key);
        if let Some(mut set) = self.by_canonical.get_mut(&key.canonical_id) {
            set.remove(key);
            if set.is_empty() {
                drop(set);
                self.by_canonical.remove(&key.canonical_id);
            }
        }
    }

    /// Evict every content-addressed entry published for `canonical`.
    ///
    /// The targeted per-file invalidation surface for this node, paired
    /// with the session node's `clear_compile_outputs_for_file`: an
    /// explicit `invalidate_compile_slots` / file-removal caller flushes
    /// the content-addressed entries for the canonical so a subsequent
    /// `Content` request recompiles instead of serving a warm entry. A
    /// content key carries no fact rail, so without this eviction a
    /// same-content recompile after invalidation would warm-hit and
    /// report `cache_hit = true`, breaking the force-recompute contract.
    pub(crate) fn remove_canonical(&self, canonical: &str) {
        if let Some((_, keys)) = self.by_canonical.remove(canonical) {
            for key in keys {
                self.entries.remove(&key);
            }
        }
    }

    /// Drop every content-addressed entry. Used by whole-store
    /// invalidation callers (e.g. [`crate::VerterHost::clear_compile_cache`])
    /// so a cache clear flushes the content-addressed node alongside the
    /// per-profile session slots.
    pub(crate) fn clear_all(&self) {
        self.entries.clear();
        self.by_canonical.clear();
    }

    /// Entry count for the content-addressed store. Used by integration
    /// tests that verify content-mode publishes vs session-mode publishes
    /// are disjoint. Available under `test` / `debug_assertions` so an
    /// external `tests/` crate (which links the lib in the dev profile)
    /// can observe the store footprint via a host accessor.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for CompileOutputNodePureContent {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtifactNode for CompileOutputNodePureContent {
    type Key = CompileOutputPureContentKey;
    type Value = Arc<CompileOutputValue>;

    fn entries(&self) -> &DashMap<Self::Key, Arc<CacheEntry<Self::Value>>> {
        &self.entries
    }

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    /// Pure-content nodes do NOT cold-build through the substrate;
    /// the production callsite (`virtual_file_pipeline.rs`) builds
    /// the value inline and admits through [`Self::publish_content`].
    /// The trait arm returns a [`CacheAdmission::Failed`] so that
    /// any future `cache_runtime::lookup` consumer cannot silently
    /// short-circuit through an unimplemented compute path —
    /// callers that need cold-build routing must wire the closure
    /// at the call site.
    fn compute(&self, _key: &Self::Key, _cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        CacheAdmission::Failed {
            reason: verter_audit::NonAdmissionReason::ComputeFailed,
        }
    }

    /// Validate a published entry against the caller's view. For
    /// pure-content mode this is a trivial generation gate — the
    /// env-hash dimensions in the key already discriminate every
    /// observable env-state change.
    fn validate(
        &self,
        _key: &Self::Key,
        entry: &CacheEntry<Self::Value>,
        cx: &ComputeCtx<'_>,
    ) -> Option<Self::Value> {
        if entry.validated_at_generation == cx.generation() {
            Some(entry.value.clone())
        } else {
            None
        }
    }
}

// ── FactValidatedSession node ─────────────────────────────────────────

/// Query-identity compile-output cache node backed by the per-profile
/// [`ProfileState::compile_slots`] table.
///
/// The node owns the `lookup` / `publish` / `remove` typed methods
/// every caller in `virtual_file_pipeline.rs` consults. Direct
/// `compile_slots` access from outside this module is forbidden —
/// the field stays private to the typed-node module and every read /
/// write routes through the methods below.
///
/// Warm-hit validation runs the path-precise fact oracle inside this
/// node's [`Self::lookup`] impl: the caller supplies a validator
/// closure that checks every recorded fact against its live store
/// view, and the node gates the warm hit on that closure plus the
/// cheaper own-content / override-hash predicates.
pub(crate) struct CompileOutputNodeFactValidatedSession {
    inflight: InflightTable<QueryFlightKey<CompileOutputSessionKey>>,
}

impl CompileOutputNodeFactValidatedSession {
    /// Construct a fresh node. The node holds no entry storage of its
    /// own — entries live on the host's [`ProfileState::compile_slots`]
    /// map and are addressed through `(canonical, profile_hash)` at
    /// every method.
    pub(crate) fn new() -> Self {
        Self {
            inflight: InflightTable::new(),
        }
    }

    /// Warm-hit lookup over the per-canonical [`ProfileState`].
    ///
    /// Returns `Some(value)` only when:
    /// 1. A slot exists for `profile_hash`.
    /// 2. The slot's carrier is `Cacheable` (i.e. not an overflowed
    ///    signature that snuck in).
    /// 3. The slot's `semantic_hash`, `style_override_hash`, and
    ///    `content_override_hash` match the supplied references.
    /// 4. `acquire_view` yields a proven-current view (it returns
    ///    `None` when the manager could not prove the view current,
    ///    which misses to cold).
    /// 5. The slot's path-precise fact signature validates against that
    ///    view (`validate_facts`), when the fact rail is non-empty.
    ///
    /// `acquire_view` is the cost gate. The caller threads the
    /// (potentially expensive) store-view read through it, and this
    /// method invokes it at most once — and ONLY after the cheap
    /// predicates (steps 1–3) confirm there is a real candidate slot
    /// worth validating. A cold miss (no slot), an overflowed carrier,
    /// or a hash mismatch returns before `acquire_view` runs, so those
    /// paths never pay for the view read.
    ///
    /// `acquire_view` runs whether or not the fact rail is empty: an
    /// empty-fact slot still tracks the owning file's own content
    /// through `semantic_hash`, but a warm hit returns the cached
    /// compile output to the caller with no outer publish / is_stable
    /// fence, so the currentness proof gates the hit unconditionally. A
    /// non-empty fact rail additionally walks `validate_facts` against
    /// the acquired view; that closure is invoked at most once and only
    /// after `acquire_view` has yielded a view.
    pub(crate) fn lookup<V, A, F>(
        &self,
        profile_state: &ProfileState,
        profile_hash: u64,
        live_semantic_hash: &Hash16,
        live_style_override_hash: u64,
        live_content_override_hash: u64,
        acquire_view: A,
        validate_facts: F,
    ) -> Option<SessionLookupHit>
    where
        A: FnOnce() -> Option<V>,
        F: FnOnce(&V, &ReadSetSignature) -> bool,
    {
        let slot = profile_state.compile_slot_for_node(profile_hash)?;
        // Carrier-defence: overflowed slots must never satisfy a warm
        // read — the cold-build producer refuses to publish them.
        // Double-sided enforcement (producer refuses; lookup refuses)
        // prevents a regression of either side from accepting stale
        // warm hits.
        if !slot.fact_dep_signature.is_cacheable() {
            return None;
        }
        if slot.semantic_hash != *live_semantic_hash
            || slot.style_override_hash != live_style_override_hash
            || slot.content_override_hash != live_content_override_hash
        {
            return None;
        }
        // The cheap predicates passed → there is a candidate slot worth
        // validating. ONLY NOW pay for the store-view read; a
        // non-current read (`None`) misses to cold.
        let view = acquire_view()?;
        if !slot.fact_dep_signature.facts.is_empty()
            && !validate_facts(&view, &slot.fact_dep_signature)
        {
            return None;
        }
        Some(SessionLookupHit {
            outputs: slot.outputs.clone(),
            diagnostics: slot.diagnostics.clone(),
            tsx: slot.tsx.clone(),
        })
    }

    /// Publish a freshly compiled value into the session slot. Routes
    /// through the `SignatureAdmission` carrier: `Cacheable` publishes
    /// the slot under the path-precise signature; `NonCacheable`
    /// (overflow / forced refusal / budget exceeded) refuses
    /// admission AND removes any prior slot for the same
    /// `(canonical, profile_hash)` so the carrier invariant `present
    /// in compile_slots ⇒ admitted cacheable entry` holds across
    /// re-computes.
    ///
    /// Returns `SessionPublishOutcome::Admitted` when the slot was
    /// published, or `SessionPublishOutcome::Refused(reason)` when
    /// admission was refused.
    pub(crate) fn publish(
        &self,
        profile_state: &mut ProfileState,
        profile_hash: u64,
        admission: SignatureAdmission,
        value: CompileOutputValue,
        last_access_tick: u64,
    ) -> SessionPublishOutcome {
        match admission {
            SignatureAdmission::Cacheable(signature) => {
                let slot = CompileSlot {
                    semantic_hash: value.semantic_hash,
                    style_override_hash: value.style_override_hash,
                    content_override_hash: value.content_override_hash,
                    outputs: value.outputs,
                    diagnostics: value.diagnostics,
                    last_good_outputs: value.last_good_outputs,
                    last_access_tick,
                    tsx: value.tsx,
                    template_analysis: value.template_analysis,
                    fact_dep_signature: signature,
                };
                profile_state.compile_slot_insert_for_node(profile_hash, slot);
                SessionPublishOutcome::Admitted
            }
            SignatureAdmission::NonCacheable(reason) => {
                profile_state.compile_slot_remove_for_node(profile_hash);
                SessionPublishOutcome::Refused(reason)
            }
        }
    }

    /// Read-only peek for the fact-signature of a session slot. Used
    /// by tests and external observability surfaces to verify the
    /// producer recorded the expected cross-file fact set.
    pub(crate) fn peek_signature(
        &self,
        profile_state: &ProfileState,
        profile_hash: u64,
    ) -> Option<ReadSetSignature> {
        profile_state
            .compile_slot_for_node(profile_hash)
            .map(|slot| slot.fact_dep_signature.clone())
    }

    /// Read-only access to a session slot's combined IDE / LSP TSX
    /// output, when present. Used by `get_ide` to satisfy the IDE
    /// surface without exposing the slot directly.
    pub(crate) fn peek_tsx(
        &self,
        profile_state: &ProfileState,
        profile_hash: u64,
    ) -> Option<CachedTsx> {
        profile_state
            .compile_slot_for_node(profile_hash)
            .and_then(|slot| slot.tsx.clone())
    }

    /// Read-only access to the last-good outputs, when present. Used
    /// by `DevServeLastKnownGood` fallback paths.
    ///
    /// The last-good rail rides on the same fact-validated slot as the
    /// warm-hit candidate, so it is gated by the SAME read-side fact
    /// validation as [`Self::lookup`]: a cross-file edit that
    /// invalidates the slot's recorded [`ReadSetSignature`] takes the
    /// last-good fallback down with it. Without this gate, a compile
    /// that fails BECAUSE a dependency changed (e.g. an imported macro
    /// type edited to an invalid shape) would serve the pre-edit output
    /// instead of surfacing the failure — a stale serve of
    /// known-invalidated semantic inputs. The override / semantic-hash
    /// pre-filter is intentionally NOT applied here: a same-content
    /// request whose overrides diverged is exactly the legitimate
    /// last-good consumer, while same-canonical source edits already
    /// clear the slot eagerly at upsert.
    pub(crate) fn peek_last_good<F>(
        &self,
        profile_state: &ProfileState,
        profile_hash: u64,
        validate_facts: F,
    ) -> Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>
    where
        F: FnOnce(&ReadSetSignature) -> bool,
    {
        let slot = profile_state.compile_slot_for_node(profile_hash)?;
        // Carrier-defence, mirroring `lookup`: an overflowed slot must
        // never satisfy any warm read (the producer refuses to publish
        // them; this is the read-side half of the double-sided guard).
        if !slot.fact_dep_signature.is_cacheable() {
            return None;
        }
        if !slot.fact_dep_signature.facts.is_empty() && !validate_facts(&slot.fact_dep_signature) {
            return None;
        }
        slot.last_good_outputs.clone()
    }

    /// Read-only access to the full set of outputs for a given
    /// virtual-node kind. Used by the warm-hit fast path in
    /// `get_virtual_file`.
    pub(crate) fn peek_output(
        &self,
        profile_state: &ProfileState,
        profile_hash: u64,
        node_kind: &VirtualNodeKind,
    ) -> Option<(CachedVirtualFile, DiagnosticsSnapshot)> {
        let slot = profile_state.compile_slot_for_node(profile_hash)?;
        let output = slot.outputs.get(node_kind)?.clone();
        Some((output, slot.diagnostics.clone()))
    }

    /// Remove the session slot for `profile_hash`. Used when an
    /// upstream invalidation drops the per-profile entry.
    pub(crate) fn remove(&self, profile_state: &mut ProfileState, profile_hash: u64) {
        profile_state.compile_slot_remove_for_node(profile_hash);
    }

    /// Drop every per-profile compile-output slot for the file the
    /// `profile_state` belongs to. Used by whole-file invalidation
    /// callers (source-content change, file eviction, reverse-dep
    /// sweep, explicit cache clear).
    ///
    /// Clears ONLY the compile-output slots. The caller retains
    /// ownership of the sibling override maps, the latest-diagnostics
    /// map, and the diagnostics-generation counter — those are compile
    /// inputs / observable state outside this node's authority, and the
    /// invalidation caller clears or bumps them as its own logic
    /// requires.
    pub(crate) fn clear_compile_outputs_for_file(&self, profile_state: &mut ProfileState) {
        profile_state.compile_slots_clear_for_node();
    }

    /// Read-only access to a session slot's template-analysis snapshot,
    /// when present. Used by the CSS-variable flow analysis to read the
    /// override-derived template analysis without exposing the slot.
    pub(crate) fn peek_template_analysis(
        &self,
        profile_state: &ProfileState,
        profile_hash: u64,
    ) -> Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot> {
        profile_state
            .compile_slot_for_node(profile_hash)
            .and_then(|slot| slot.template_analysis.clone())
    }
}

impl Default for CompileOutputNodeFactValidatedSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a successful [`CompileOutputNodeFactValidatedSession::lookup`].
pub(crate) struct SessionLookupHit {
    /// Per-virtual-node-kind outputs.
    pub outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
    /// Diagnostics snapshot from compile time.
    pub diagnostics: DiagnosticsSnapshot,
    /// Optional combined TSX output (IDE / LSP).
    pub tsx: Option<CachedTsx>,
}

/// Result of [`CompileOutputNodeFactValidatedSession::publish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionPublishOutcome {
    /// Slot was published under a `Cacheable` admission.
    Admitted,
    /// Admission refused. The carried reason classifies why.
    Refused(verter_audit::NonAdmissionReason),
}

impl QueryNode for CompileOutputNodeFactValidatedSession {
    type Key = CompileOutputSessionKey;
    type Discriminant = FactCandidateDiscriminant;
    type Value = Arc<CompileOutputValue>;

    fn inflight(&self) -> &InflightTable<QueryFlightKey<Self::Key>> {
        &self.inflight
    }

    /// Session-mode candidate lookup. Defers to the host-side
    /// `lookup` method which inspects the per-profile compile slot
    /// against the caller's live override / semantic hashes. The
    /// trait-level entry point is intentionally a no-op stub at the
    /// substrate boundary — production callers route through the
    /// concrete `lookup` method on this type with the live hash
    /// references they have at hand. The arm returns `None` so any
    /// future `query::lookup` consumer cannot silently short-circuit
    /// through an unrouted lookup path.
    fn lookup_candidate(&self, _key: &Self::Key, _cx: &ComputeCtx<'_>) -> Option<Self::Value> {
        None
    }

    /// Session-mode cold-build is owned by `virtual_file_pipeline.rs`;
    /// the typed node admits the freshly built value through
    /// [`Self::publish`]. The substrate-trait arm returns
    /// [`CacheAdmission::Failed`] so an unrouted `query::lookup`
    /// consumer cannot silently short-circuit.
    fn compute(&self, _key: &Self::Key, _cx: &mut ComputeCtx<'_>) -> CacheAdmission<Self::Value> {
        CacheAdmission::Failed {
            reason: verter_audit::NonAdmissionReason::ComputeFailed,
        }
    }

    /// Discriminant for an admitted session candidate.
    ///
    /// Same-discriminant re-publishes (under the same view's
    /// generation and observed fact set) replace in place; any
    /// difference admits a distinct candidate up to the slot's
    /// candidate cap.
    fn discriminant(
        &self,
        _key: &Self::Key,
        _value: &Self::Value,
        signature: &ReadSetSignature,
        validated_at_generation: u64,
    ) -> Self::Discriminant {
        FactCandidateDiscriminant {
            validated_at_generation,
            facts: signature.facts.clone(),
        }
    }

    /// Session-mode publish-core is not routed through the substrate's
    /// `query::lookup` yet — production publish flows through
    /// [`Self::publish`] above. The arm returns a rejection so an
    /// unrouted `query::lookup` consumer cannot silently advance a
    /// FIFO budget.
    fn publish_core(
        &self,
        _key: Self::Key,
        _candidate: Candidate<Self::Discriminant, Self::Value>,
    ) -> PublishCoreOutcome<Self::Key> {
        PublishCoreOutcome {
            outcome: PublishOutcome::Rejected(verter_audit::NonAdmissionReason::ComputeFailed),
            deferred_victims: DeferredVictims::new(),
        }
    }
}

#[cfg(test)]
#[path = "compile_output_node_tests.rs"]
mod tests;
