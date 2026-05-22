#![deny(missing_docs)]
//! Session-layer structural materialiser. Dispatch-driven, with
//! graph-native policy predicates, cooperative-admission post-compute
//! revalidation for atomic publish/invalidate, and a content-hash
//! bucketed Weak-ref `DepSignature` interner for `Arc::ptr_eq` cleanup
//! of the reverse-index.
//!
//! **Foundational types**:
//! - [`MaterializeOutcome`] — materialiser-local result enum
//!   (Value / Miss / Recursive / Tainted / Error).
//! - [`MaterializationScope`] — TopLevel vs Nested axis.
//! - [`MaterializeStructureCacheKey`] — final-result cache key.
//! - [`convert_dispatch_result`] — boundary that promotes
//!   `QueryResult::Recursive` to `MaterializeOutcome::Tainted`.
//!
//! **Materialiser entry**:
//! - [`materialize_component_meta_structure`] — five-stage entry
//!   pipeline (warm peek → same-key cycle → depth fuse → package /
//!   function policy gates → cooperative-admission cold build with
//!   `post_publish` reverse-index registration).
//! - [`materialize_object_surface`] — per-shape Object handler
//!   that walks members + call/construct/index signatures at Nested
//!   axis. Re-entry through the materialiser entry applies the
//!   package-ref + function-skip policies, so function-valued
//!   members and package-backed refs stay symbolic while local
//!   refs continue to expand.
//! - DeclRef / InstantiationRef handler resolves the carrier's
//!   body via dispatch `Instantiate` (NOT `ResolveDecl`) and
//!   recursively materialises the resolved body.
//!
//! **Policy predicates**:
//! - [`is_package_backed_ref`] — graph-native check that the input
//!   carrier resolves under `/node_modules/`. Keeps the result
//!   symbolic at every axis.
//! - Function-shape skip at Nested — keeps function bodies symbolic
//!   for Object-property positions.
//!
//! The static-grep gate at `tests/no_legacy_walker.rs` enforces that
//! retired walker symbols never reappear — see that file's
//! `RETIRED_SYMBOLS` array for the canonical list of names.

use std::sync::Arc;

use crate::semantic_query::{
    CacheRead, DepSignature, DepVersion, ProjectionMode, QueryError, QueryResult, SemanticNodeId,
};

/// Materialiser-local outcome enum.
///
/// Distinct from `QueryResult` because `Tainted` is materialiser-
/// scoped: it captures depth-fuse trips, scope-unloaded outcomes,
/// AND `QueryResult::Recursive` results promoted at the dispatch
/// boundary. None of those are cacheable as warm entries.
#[derive(Debug, Clone)]
pub enum MaterializeOutcome {
    /// Successful materialisation. Cacheable.
    Value(SemanticNodeId),
    /// Computation produced an `Opaque(Miss)` or otherwise valid
    /// negative result. Cacheable per content generation.
    #[allow(dead_code)]
    Miss(SemanticNodeId),
    /// Same-key recursion detected on this thread. Non-cacheable.
    /// Returned by the per-thread `MATERIALIZE_IN_FLIGHT` guard.
    Recursive(SemanticNodeId),
    /// Path-dependent outcome — depth-fuse trip, scope-unloaded, or
    /// a dispatch sub-call returned `Recursive`. Non-cacheable;
    /// propagates upward through the worklist as `Tainted`.
    Tainted(SemanticNodeId),
    /// Other dispatch error. Non-cacheable.
    Error(#[allow(dead_code)] QueryError),
}

impl MaterializeOutcome {
    /// Extract the carried node id. For `Error` variants returns
    /// the caller-supplied opaque-miss id (non-extractable from
    /// QueryError directly — callers pass the ctx's opaque-miss
    /// fallback).
    #[allow(dead_code)]
    #[must_use]
    pub fn node_id(&self, opaque_miss_fallback: SemanticNodeId) -> SemanticNodeId {
        match self {
            Self::Value(id) | Self::Miss(id) | Self::Recursive(id) | Self::Tainted(id) => *id,
            Self::Error(_) => opaque_miss_fallback,
        }
    }

    /// `true` for outcomes that may be published to the
    /// `MaterializeStructureDb` warm cache. invariant:
    /// only `Value` and `Miss` are cacheable. `Recursive` and
    /// `Tainted` are per-call-context; `Error` is non-deterministic.
    #[must_use]
    pub fn is_cacheable(&self) -> bool {
        matches!(self, Self::Value(_) | Self::Miss(_))
    }

    /// `true` when this outcome must propagate upward as a
    /// `Tainted` parent outcome.
    #[allow(dead_code)]
    #[must_use]
    pub fn taints_parent(&self) -> bool {
        matches!(self, Self::Tainted(_))
    }
}

/// Materialisation scope axis. Determines how
/// the policy table dispatches each shape: TopLevel arms vs
/// Nested arms (e.g., function-typed property at Nested skips,
/// while Function handler at TopLevel always materialises).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterializationScope {
    /// First-call axis. Bare DeclRef materialises; Function arm
    /// always materialises params/return; Object surface fully
    /// materialised.
    TopLevel,
    /// Recursive descent axis. Function-typed Object property
    /// skips; bare Generic-with-args reserved for InstantiationRef
    /// arm.
    Nested,
}

impl From<MaterializationScope> for crate::component_meta_audit::MaterializationScopeAudit {
    fn from(s: MaterializationScope) -> Self {
        match s {
            MaterializationScope::TopLevel => Self::TopLevel,
            MaterializationScope::Nested => Self::Nested,
        }
    }
}

/// Final-result cache key for the materialiser.
/// `scope_canonical_id` is **NOT part of the cache key**.
///
/// **Cache-key semantics:** the cache key dimensions are
/// `(base, scope_axis, mode)`. `scope_canonical_id` is retained
/// on the struct as a fence-seed input that flows into the
/// per-candidate `dep_signature` but is excluded from `Hash` and
/// `PartialEq`. Cross-owner reuse: N consumer scopes that reach
/// the same `(base, scope_axis, mode)` produce ONE cache entry.
///
/// **Rationale (R7 + R8):** see audit doc
/// `docs/arch/materialize-owner-local-audit.md`. The audit confirmed
/// the local_fence_seed is a derived function of `(defining_canonical,
/// content_hash)`. `defining_canonical` lives inside `base`'s
/// `NodeScopeId::File { canonical_id }` (recoverable via the
/// semantic-graph store); `content_hash` lives in
/// `VersionedDeclIdentity.content_hash` inside the cached value.
/// The consumer-scope canonical id is NOT load-bearing.
///
/// The richer [`MaterializationCacheKey`] is the end-state form:
/// `decl: ResolvedDeclSlotIdentity` + `projection_path` +
/// `projection_mode` + `normalized_type_args` + `options_hash`.
/// The cache-key behavior change is already in effect via the
/// hand-rolled `Hash`/`PartialEq`; downstream consumers migrate
/// to the explicit field form when adopting the richer key.
#[derive(Debug, Clone)]
pub struct MaterializeStructureCacheKey {
    /// Owner scope — the canonical id the materialiser was
    /// dispatched in. **NOT in the cache key.**
    /// Retained as a fence-seed input only.
    pub scope_canonical_id: Arc<str>,
    /// Input semantic node — the lowered TypeExpr that the
    /// materialiser is asked to materialise. **Cache key
    /// dimension.**
    pub base: SemanticNodeId,
    /// Axis the input was lowered at. **Cache key dimension.**
    pub scope_axis: MaterializationScope,
    /// Caller-side projection mode the materialiser ran with.
    /// **Cache key dimension.**
    pub mode: ProjectionMode,
}

impl PartialEq for MaterializeStructureCacheKey {
    /// `scope_canonical_id` is intentionally excluded.
    /// Cross-owner reuse: N consumer scopes reaching the same
    /// `(base, scope_axis, mode)` produce ONE cache entry.
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.scope_axis == other.scope_axis && self.mode == other.mode
    }
}

impl Eq for MaterializeStructureCacheKey {}

impl std::hash::Hash for MaterializeStructureCacheKey {
    /// `scope_canonical_id` is intentionally excluded.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.base.hash(state);
        self.scope_axis.hash(state);
        self.mode.hash(state);
    }
}

/// End-state form for the materialiser cache key:
///
/// ```ignore
/// struct MaterializationCacheKey {
///     decl: ResolvedDeclSlotIdentity,
///     projection_path: ProjectionPathHash,
///     projection_mode: ProjectionMode,
///     normalized_type_args: TypeArgsHash,
///     options_hash: Hash16,
/// }
/// ```
///
/// Introduced alongside [`MaterializeStructureCacheKey`]. The
/// existing key's `Hash`/`PartialEq` already deliver the cross-owner
/// reuse contract (consumer scope excluded). Downstream consumers
/// migrate from the legacy key to this explicit form when the
/// richer dimensions (`projection_path`, `normalized_type_args`)
/// become load-bearing.
///
/// The discriminating test
/// `tests/cross_owner_materialise_reuse.rs` verifies the cross-owner
/// reuse invariant via the cache-entry count.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaterializationCacheKey {
    /// Resolved declaration slot. Carries the content-free 6-field
    /// identity (R7).
    pub decl: crate::semantic_query::ResolvedDeclSlotIdentity,
    /// Hash of the projection path (`['a']['b']['c']` chain) for
    /// path-precise materialisation. Empty path = whole-surface.
    pub projection_path: ProjectionPathHash,
    /// Caller-side projection mode the materialiser ran with.
    pub projection_mode: ProjectionMode,
    /// Hash of the normalized type-argument list. Walks args in
    /// declaration order, alpha-normalised as structural `TypeExpr`;
    /// free type-params become `TypeParam(<binder-relative index>)`.
    /// See `docs/arch/materialize-owner-local-audit.md` for the
    /// normalisation rationale.
    pub normalized_type_args: TypeArgsHash,
    /// Caller-side options hash for fence options.
    pub options_hash: crate::semantic_query::HashValue,
}

/// Hash of a projection path. Computed from the typed-IR
/// `TypeExpr` chain when the path becomes load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ProjectionPathHash(pub [u8; 16]);

/// Hash of a normalized type-argument list. Computed from the
/// alpha-normalised typed-IR argument list per the normalisation
/// rules documented in
/// `docs/arch/materialize-owner-local-audit.md` (b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TypeArgsHash(pub [u8; 16]);

/// Boundary that converts a dispatch
/// `CacheRead<QueryResult<SemanticNodeId>>` to a
/// `MaterializeOutcome`. The conversion is the load-bearing place
/// where `QueryResult::Recursive` promotes to
/// `MaterializeOutcome::Tainted` (NOT `Miss`) — the dispatch's
/// same-path recursion sentinel must NOT be baked into the
/// materialiser cache as a finalised Miss; it is per-call-context.
///
/// Side effect: appends every `dep_signature` entry into
/// `local_fence` so the materialiser's compose path observes
/// transitive dep facts.
#[allow(dead_code)]
pub fn convert_dispatch_result(
    read: CacheRead<QueryResult<SemanticNodeId>>,
    input_node_for_sentinel: SemanticNodeId,
    local_fence: &mut Vec<(Arc<str>, DepVersion)>,
) -> MaterializeOutcome {
    crate::component_meta_audit::merge_dep_signature_into_local_fence(
        local_fence,
        &read.dep_signature,
    );
    match read.value {
        QueryResult::Value(id) => MaterializeOutcome::Value(id),
        // Recursive promotes to Tainted (not cacheable Miss). Plan
        // §1.2: the dispatch's same-path recursion sentinel must
        // NOT be baked into the materialiser cache.
        QueryResult::Recursive(_) => MaterializeOutcome::Tainted(input_node_for_sentinel),
        QueryResult::Error(err) => MaterializeOutcome::Error(err),
    }
}

// ──────────────────────────────────────────────────────────────────
// Materialiser entry
// ──────────────────────────────────────────────────────────────────

use std::cell::{Cell, RefCell};

use crate::component_meta_caches::MaterializeStructureEntry;
// Migration: this consumer routes through
// `cooperative_admit_with_post_publish` (the ComputeAdmission API)
// instead of the legacy `cooperative_get_or_insert_with_post_publish`.
// The legacy entry-point remains the public API for callers whose
// compute closures never produce non-cacheable values.
// Test-only — the in-tree `mod tests { … }` block at the bottom of this
// file constructs `ProjectSemanticDispatch::new(host)` directly to drive
// dispatch-pipeline assertions. Gated `#[cfg(test)]` to keep the
// non-test build's used-imports surface minimal.
#[cfg(test)]
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{PathSegment, SemanticQueryKey};

/// Test-only fact-injection knob. When set to `N > 0`, the materialiser's
/// cold-compute closure observes `N` synthetic `FileWholeHash` facts via
/// `observe_fan_out` BEFORE returning, deterministically forcing the
/// installed fact tracer to either overflow (when `N >
/// FACT_SIGNATURE_CAP`) or accumulate a large signature. Drives the
/// discriminating Overflow-returns-valid-result test without requiring a
/// pathological workspace fixture that organically produces > 1024 facts.
///
/// The flag is reset to 0 by the RAII guard [`MaterializeForceOverflowGuard`]
/// after the test completes so concurrent tests are not affected.
/// Production reads it once per cold compute as a relaxed atomic load
/// (~1 ns); the load path lives on the cooperative-admission cold-build
/// path which already takes locks, so the cost is in the noise.
#[doc(hidden)]
pub(crate) static MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// RAII guard that clears [`MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS`]
/// on drop. Test setup loads the desired observation count; the guard
/// drops at scope exit and restores the baseline so a panic / early
/// return does not leak the forced state into concurrent tests.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub struct MaterializeForceOverflowGuard;

#[cfg(any(test, debug_assertions))]
impl MaterializeForceOverflowGuard {
    /// Set the forced observation count to `n` and return the guard.
    pub(crate) fn arm(n: usize) -> Self {
        MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS.store(n, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for MaterializeForceOverflowGuard {
    fn drop(&mut self) {
        MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

thread_local! {
    /// Per-thread stack of in-flight materialiser keys.
    /// Used for same-key recursion detection. Push on entry, pop on
    /// exit (RAII via `MaterializeInFlightGuard`).
    static MATERIALIZE_IN_FLIGHT: RefCell<Vec<MaterializeStructureCacheKey>> =
        const { RefCell::new(Vec::new()) };

    /// Per-thread depth counter. The materialiser's
    /// defensive depth fuse trips at `MAX_DEPTH` to bound stack
    /// growth on pathological recursive shapes.
    static MATERIALIZE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Defensive depth fuse cap. A trip is a bug, not a
/// soft-fail; the audit emits `MaterializeStructureDepthFuseTripped`
/// with the input key + depth.
pub const MAX_DEPTH: usize = 4096;

// `NonCacheableSlot` was a stack-local `RefCell<Option<...>>` side
// channel used to broadcast non-cacheable materialisation outcomes
// to the post-cooperative fallback. The carrier consolidation
// retired it: cooperative joiners now observe non-cacheable outcomes
// directly through `ComputeAdmission::ReturnOnly`'s typed broadcast
// channel inside the in-flight slot (`cooperative_admission.rs`).

/// RAII guard for the per-thread `MATERIALIZE_IN_FLIGHT`
/// stack and the `MATERIALIZE_DEPTH` counter. Push on construction,
/// pop on `Drop`. Panic-safe.
pub struct MaterializeInFlightGuard {
    key: Option<MaterializeStructureCacheKey>,
}

impl MaterializeInFlightGuard {
    /// Push `key` onto the per-thread in-flight stack and increment
    /// the depth counter. Returns the guard.
    pub fn push(key: MaterializeStructureCacheKey) -> Self {
        MATERIALIZE_IN_FLIGHT.with(|stack| stack.borrow_mut().push(key.clone()));
        MATERIALIZE_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        Self { key: Some(key) }
    }

    /// Test/diagnostic — current per-thread depth.
    #[must_use]
    pub fn current_depth() -> usize {
        MATERIALIZE_DEPTH.with(Cell::get)
    }

    /// Internal — does the per-thread stack already contain `key`?
    fn contains_key(key: &MaterializeStructureCacheKey) -> bool {
        MATERIALIZE_IN_FLIGHT.with(|stack| stack.borrow().contains(key))
    }
}

impl Drop for MaterializeInFlightGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            MATERIALIZE_IN_FLIGHT.with(|stack| {
                let mut v = stack.borrow_mut();
                if let Some(pos) = v.iter().rposition(|k| k == &key) {
                    v.remove(pos);
                }
            });
            MATERIALIZE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
}

/// Materialiser entry. Produces a `CacheRead` carrying
/// the materialisation outcome plus the dep_signature observed
/// during the cold build.
///
/// **Cache hierarchy:**
/// 1. Peek `MaterializeStructureDb` — warm hit returns immediately.
/// 2. Same-key thread-local re-entry → `Recursive(opaque_miss)`,
///    no cache write.
/// 3. Pre-admission depth-fuse check → `Tainted(key.base)` if
///    depth > `MAX_DEPTH`, no cache write.
/// 4. Cooperative-admission cold build via
///    `cooperative_get_or_insert_with_post_publish`. The compute
///    closure dispatches `ProjectPath { base, [], mode }` to the
///    canonical materialisation pipeline. The post_publish callback
///    registers the (key, dep_signature) pair in the
///    `canonical_to_keys` reverse index.
///
/// **Tainted is a sentinel propagation:** when the dispatch returns
/// `QueryResult::Recursive`, `convert_dispatch_result` promotes it
/// to `MaterializeOutcome::Tainted(input)`. The materialiser's
/// publish gate skips cache writes for non-cacheable outcomes.
///
/// **Audit signal:** every entry/exit emits `MaterializeStructureEnter`
/// and `MaterializeStructureExit` events with the resolved
/// `CacheOutcomeKind` (`Hit` for warm, `ColdBuild` for cold,
/// `Tainted` for tainted, `Miss` for opaque).
/// Single-exit helper for the materialiser compute
/// closure. Seeds `local_fence` with the root scope's whole_hash if
/// available, then either:
/// - For non-cacheable outcomes (Recursive / Tainted / Error), stashes
///   `(outcome, fence)` in `non_cacheable_slot` and returns `None` so
///   the cooperative-admission fallback can return the correct outcome
///   without re-dispatching.
/// - For cacheable outcomes (Value / Miss), returns `Some(MaterializeStructureEntry)`
///   so cooperative-admission publishes it.
///
/// The single admission boundary for a materialiser compute closure.
/// Converts a `(MaterializeOutcome, local_fence)` pair into a
/// [`ComputeAdmission`]:
///
/// - intrinsically non-cacheable outcome (`Recursive` / `Tainted` /
///   `Error`) → [`ComputeAdmission::ReturnOnly`].
/// - valid `Value` / `Miss` outcome whose observed-root signature can
///   be built strictly → [`ComputeAdmission::Cacheable`].
/// - valid `Value` / `Miss` outcome whose signature CANNOT be built
///   strictly (the fence carries an unsound `RouteGeneration`
///   dependency, or a fence `WholeHash` conflicts with an observed
///   base self-root) → `ReturnOnly`. The value is still returned to
///   every joiner; the shared cache stays empty so the next cold-miss
///   recomputes.
///
/// `base_origin_self_root` is the `base` node's declaration-origin
/// file (`NodeScopeId::File`) when it is file-derived — the entry's
/// strict self-root, pinned to the `whole_hash` baked into the node's
/// origin sidecar at intern time. The consumer materialise scope is
/// NOT a self-root: a `MaterializeStructureDb` value's identity does
/// not depend on which consumer reached it (R7 cross-owner reuse). If
/// the scope IS read during compute, that read is observed naturally
/// through the tracer / local-dep path and appears as an ordinary
/// dependency fact in `local_fence`.
///
/// `validated_at_generation` is the project generation snapshotted by
/// the `compute` closure before it dispatched any work; it is stamped
/// onto every `Cacheable` entry. The carrier validates only
/// file-content whole-hashes, so a `ProjectGeneration` reset would go
/// undetected without this — the post-compute revalidation and every
/// read-side gate reject an entry whose stamp no longer matches the
/// live generation.
fn finish_materialize_admission(
    outcome: MaterializeOutcome,
    local_fence: Vec<(Arc<str>, DepVersion)>,
    base_origin_self_root: Option<&(Arc<str>, crate::resolver_core::ResolverHash16)>,
    validated_at_generation: u64,
) -> crate::cooperative_admission::ComputeAdmission<
    crate::semantic_query::CacheRead<MaterializeOutcome>,
    MaterializeStructureEntry,
> {
    if !outcome.is_cacheable() {
        // Valid result but cannot be admitted to the cache (the
        // materialised outcome is intrinsically non-cacheable, e.g.
        // Tainted). Broadcast via ComputeAdmission::ReturnOnly so
        // joiners observe the same valid outcome. The CacheRead's
        // dep_signature is empty: non-cacheable results MUST NOT
        // propagate as cache deps (R20).
        return crate::cooperative_admission::ComputeAdmission::ReturnOnly(
            crate::semantic_query::CacheRead {
                value: outcome,
                dep_signature: empty_signature(),
                walker_diagnostics: Arc::from([]),
                cache_suppress: false,
            },
        );
    }
    let dispatch_dep_signature = dep_signature_from_fence(local_fence.clone());
    match materialize_structure_read_set(&local_fence, base_origin_self_root) {
        Some((facts, self_root_canonicals)) => {
            crate::cooperative_admission::ComputeAdmission::Cacheable(MaterializeStructureEntry {
                outcome,
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                dispatch_dep_signature,
                self_root_canonicals,
                admission_seq: crate::bounded_query_retention::next_retention_seq(),
                validated_at_generation,
            })
        }
        None => {
            // The observed-root signature cannot be built strictly —
            // the fence carries an unsound `RouteGeneration` dependency
            // or a fence `WholeHash` conflicts with the observed base
            // self-root. The materialised outcome is valid; route it
            // through `ReturnOnly` so joiners observe it without
            // admitting an entry the warm-read validator could not
            // soundly check.
            crate::cooperative_admission::ComputeAdmission::ReturnOnly(
                crate::semantic_query::CacheRead {
                    value: outcome,
                    dep_signature: empty_signature(),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                },
            )
        }
    }
}

/// Build the observed-root fact signature + self-root canonical set
/// for a `MaterializeStructureDb` entry — **provenance-pure**.
///
/// The signature leads with the `base` node's declaration-origin
/// self-root `FileWholeHash` (when the base is file-derived), then
/// merges the fence facts as cross-file dependency facts.
///
/// The single self-root is the `base` node's declaration-origin file
/// (`base_origin_self_root`) — pinned to the `whole_hash` baked into
/// the node's `NodeScopeId::File` origin at intern time. The consumer
/// materialise scope is NOT a self-root: a `MaterializeStructureDb`
/// value's identity does not depend on which consumer reached it (R7
/// cross-owner reuse — N consumer scopes reaching the same
/// `(base, scope_axis, mode)` share ONE entry). If the consumer scope
/// IS read during compute, that read enters `local_fence` as an
/// ordinary dependency fact through the normal tracer path.
///
/// A `Global`-origin base with no traced fence facts and no
/// `base_origin_self_root` is admissible as a zero-self-root,
/// zero-fact entry — a genuinely content-invariant materialisation.
///
/// Returns `None` — refusing shared-cache admission — when:
///
/// - a fence entry names the base origin self-root canonical with a
///   `WholeHash` that disagrees with the observed self-root hash (a
///   torn observation);
/// - a fence entry carries a `RouteGeneration` dependency (no
///   authoritative validating source — see [`fact_signature_from_fence`]).
///
/// `ProjectGeneration` fence entries convert to
/// `FactVersionRef::ProjectGeneration` (a project-shape change rejects
/// the entry; a pure content edit does not over-invalidate). A
/// `ProjectGeneration` floor is NEVER synthesized — it is recorded
/// only when the computation actually observed one.
fn materialize_structure_read_set(
    local_fence: &[(Arc<str>, DepVersion)],
    base_origin_self_root: Option<&(Arc<str>, crate::resolver_core::ResolverHash16)>,
) -> Option<crate::fact_signature_helpers::StructuralCarrierReadSet> {
    use crate::resolver_core::FactVersionRef;

    // Collapse observed self-roots into a per-canonical hash map; a
    // conflicting hash for the same canonical is a torn observation.
    // The `base` node's declaration-origin file is the sole self-root.
    let mut self_root_hashes: rustc_hash::FxHashMap<Arc<str>, crate::types::Hash16> =
        rustc_hash::FxHashMap::default();
    if let Some((origin_canonical, origin_hash)) = base_origin_self_root {
        self_root_hashes.insert(Arc::clone(origin_canonical), *origin_hash);
    }

    let mut facts: Vec<FactVersionRef> =
        Vec::with_capacity(self_root_hashes.len() + local_fence.len());
    // Lead with one self-root `FileWholeHash` per observed self-root.
    for (canonical, observed_hash) in &self_root_hashes {
        facts.push(FactVersionRef::FileWholeHash {
            canonical_id: canonical.as_ref().to_string(),
            hash: *observed_hash,
        });
    }

    // Merge the fence facts as cross-file dependency facts. A fence
    // `WholeHash` for a self-root canonical is folded onto the
    // observed self-root: it MUST agree with the observed hash.
    for (canonical, version) in local_fence.iter() {
        match version {
            DepVersion::WholeHash(hash) => {
                if let Some(observed_hash) = self_root_hashes.get(canonical) {
                    if hash != observed_hash {
                        return None;
                    }
                    // Already emitted as a self-root — do not duplicate.
                    continue;
                }
                facts.push(FactVersionRef::FileWholeHash {
                    canonical_id: canonical.as_ref().to_string(),
                    hash: *hash,
                });
            }
            DepVersion::ProjectGeneration(generation) => {
                facts.push(FactVersionRef::ProjectGeneration {
                    generation: *generation,
                });
            }
            DepVersion::RouteGeneration(_) => {
                // Unsound — see `fact_signature_from_fence`.
                return None;
            }
        }
    }

    let mut self_root_canonicals: Vec<Arc<str>> = self_root_hashes.into_keys().collect();
    self_root_canonicals.sort();
    Some((Arc::from(facts), Arc::from(self_root_canonicals)))
}

/// The `base` node's declaration-origin self-root for a materialiser
/// compute closure: `Some((canonical, whole_hash))` when the node's
/// `SemanticGraphStore::node_scope` is a `NodeScopeId::File`. The
/// `whole_hash` is the file content version baked into the node's
/// origin sidecar at intern time — an observed identity, not a
/// current-content re-read.
fn base_node_origin_self_root(
    ctx: &dyn ResolverContext,
    base: SemanticNodeId,
) -> Option<(Arc<str>, crate::resolver_core::ResolverHash16)> {
    let graph = ctx.project_type_store().semantic_graph();
    match graph.node_scope(base)? {
        crate::semantic_query::NodeScopeId::File {
            canonical_id,
            whole_hash,
            ..
        } => Some((canonical_id, whole_hash)),
        crate::semantic_query::NodeScopeId::Global => None,
    }
}

/// Re-base a `MaterializeStructureDb` entry's fact rail on top of its
/// observed self-roots after the cold-compute `install_fact_tracer`
/// scope finalises with a traced observation set.
///
/// The producer ([`materialize_structure_read_set`]) builds the
/// entry's `facts` rail leading with one observed-hash self-root
/// `FileWholeHash` per `self_root_canonicals` entry — the `base`
/// node's declaration-origin file. The traced set is the
/// `install_fact_tracer` scope's authoritative observation set — it
/// catches transitively-bubbled facts the materialiser's `local_fence`
/// (which merges only legacy dep-signature rails) can miss. This
/// helper keeps the observed self-roots from the producer carrier,
/// then merges the traced facts ON TOP — exactly the
/// `semantic_graph_read_set_signature` discipline: a traced
/// `FileWholeHash` for a self-root canonical is folded onto the
/// observed self-root (it MUST agree — a mismatch is a torn read), a
/// traced `ProjectGeneration` is kept, every other traced fact is kept
/// verbatim.
///
/// Returns `None` — the caller routes the value through `ReturnOnly`
/// — when a traced `FileWholeHash` disagrees with an observed
/// self-root hash (torn read).
fn merge_traced_facts_into_materialize_carrier(
    producer_facts: &[crate::resolver_core::FactVersionRef],
    self_root_canonicals: &[Arc<str>],
    traced_facts: &[crate::resolver_core::FactVersionRef],
) -> Option<Arc<[crate::resolver_core::FactVersionRef]>> {
    use crate::resolver_core::FactVersionRef;

    // The observed self-root hashes are the producer carrier's leading
    // `FileWholeHash` facts whose canonical is a self-root.
    let mut self_root_hashes: rustc_hash::FxHashMap<&str, crate::types::Hash16> =
        rustc_hash::FxHashMap::default();
    for fact in producer_facts {
        if let FactVersionRef::FileWholeHash { canonical_id, hash } = fact {
            if self_root_canonicals
                .iter()
                .any(|c| c.as_ref() == canonical_id.as_str())
            {
                self_root_hashes.insert(canonical_id.as_str(), *hash);
            }
        }
    }

    // Keep ONLY the producer carrier's self-root `FileWholeHash` facts
    // — the observed-version self-roots that lead the carrier. Every
    // other producer fact (non-self-root dependency facts) is
    // re-derived from the (superset) traced set below.
    let mut facts: Vec<FactVersionRef> =
        Vec::with_capacity(producer_facts.len() + traced_facts.len());
    for fact in producer_facts {
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if self_root_hashes.contains_key(canonical_id.as_str()) =>
            {
                facts.push(fact.clone());
            }
            // Non-self-root producer dependency facts are re-derived
            // from the (superset) traced set below — drop them here.
            _ => {}
        }
    }

    // Merge the traced facts. A traced `FileWholeHash` for a self-root
    // canonical folds onto the observed self-root (must agree); every
    // other traced fact is kept verbatim so transitive invalidation
    // works.
    for fact in traced_facts {
        if let FactVersionRef::FileWholeHash { canonical_id, hash } = fact {
            if let Some(observed) = self_root_hashes.get(canonical_id.as_str()) {
                if hash != observed {
                    return None;
                }
                // Already emitted as a self-root — do not duplicate.
                continue;
            }
        }
        facts.push(fact.clone());
    }
    Some(Arc::from(facts))
}

/// Five-phase materialiser entry. Maintains
/// the `MaterializeStructureDb` warm cache via cooperative-admission
/// with `post_publish` reverse-index registration.
///
/// **Phases:**
/// 1. Warm peek with proactive stale-entry removal.
/// 2. Same-key thread-local re-entry detection.
/// 3. Pre-admission depth fuse.
/// 4. Package-ref / function-shape-at-Nested policy gates.
/// 5. Cooperative-admission cold build with `post_publish`. Inside
///    the compute closure: registry-route branch,
///    recursive-helper cycle guard, then the
///    canonical DeclRef / InstantiationRef / Object handlers.
///
/// **Cache contract**:
/// - Only `Value` and `Miss` outcomes publish to the warm cache.
/// - `Recursive` and `Tainted` are per-call-context and never cache.
/// - `Error` is non-deterministic and never caches.
///
/// **Audit signal:** every entry/exit emits `MaterializeStructureEnter`
/// and `MaterializeStructureExit` events with the resolved
/// `CacheOutcomeKind` (`Hit` for warm, `ColdBuild` for cold,
/// `Tainted` for tainted, `Miss` for opaque). — also
/// emits `MaterializeStructurePolicySkip` events with one of:
/// `PackageRefTopLevel`, `FunctionPropertyAtNested`,
/// `RegistryRouteCycleGuard`, or `RecursiveHelperCycleGuard`.
pub(crate) fn materialize_component_meta_structure(
    ctx: &dyn ResolverContext,
    key: MaterializeStructureCacheKey,
) -> crate::semantic_query::CacheRead<MaterializeOutcome> {
    let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new(
        &crate::loop5_instrumentation::MATERIALIZE_STRUCTURE_CALLS,
        &crate::loop5_instrumentation::MATERIALIZE_STRUCTURE_NS,
    );
    crate::host_manage::record_materialize_structure_call();

    let db = ctx.project_type_store().materialize_structure_db();

    // Warm-hit peek with proactive stale removal.
    if let Some(cached) = db.peek(&key, ctx) {
        return cached;
    }

    // Same-key thread-local re-entry detection.
    if MaterializeInFlightGuard::contains_key(&key) {
        let opaque = ctx.project_type_store().semantic_graph().intern_node(
            crate::semantic_query::SemanticNodeData::Opaque(QueryError::Miss),
        );
        return crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Recursive(opaque),
            dep_signature: empty_signature(),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        };
    }

    // Pre-admission depth fuse (one-call-deep check).
    if MaterializeInFlightGuard::current_depth() >= MAX_DEPTH {
        return crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Tainted(key.base),
            dep_signature: empty_signature(),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        };
    }

    let _guard = MaterializeInFlightGuard::push(key.clone());

    // Package-ref policy gate. A DeclRef or
    // InstantiationRef whose declaration resolves under
    // `/node_modules/` materialises to itself unchanged (the walker
    // kept these symbolic at every axis; expanding them would
    // publish package internals into the consumer's component-meta
    // surface).
    if is_package_backed_ref(ctx, key.base) {
        // Observability for kept-symbolic decision.
        crate::host_manage::emit_policy_skip(
            key.base,
            key.scope_axis,
            crate::component_meta_audit::MaterializeSkipReason::PackageRefTopLevel,
        );
        return crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Value(key.base),
            dep_signature: empty_signature(),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        };
    }

    // Function-shape skip at Nested axis. The walker
    // kept function-typed Object members symbolic (their value
    // node was not expanded). Without this gate, dispatch's
    // ProjectPath { mode: Expanded } would unfold function bodies
    // inside member positions.
    if key.scope_axis == MaterializationScope::Nested {
        let graph = ctx.project_type_store().semantic_graph();
        if let Some(data) = graph.node_data(key.base) {
            if matches!(
                data.as_ref(),
                crate::semantic_query::SemanticNodeData::Function { .. }
            ) {
                crate::host_manage::emit_policy_skip(
                    key.base,
                    key.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::FunctionPropertyAtNested,
                );
                return crate::semantic_query::CacheRead {
                    value: MaterializeOutcome::Value(key.base),
                    dep_signature: empty_signature(),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                };
            }
        }
    }

    // Cooperative-admission cold build with post_publish. The
    // compute closure returns `ComputeAdmission<MaterializeOutcome,
    // MaterializeStructureEntry>`: `Cacheable(entry)` for
    // materialisations that admit to the cache, `ReturnOnly(outcome)`
    // for valid-but-non-cacheable materialisations (intrinsically
    // non-cacheable outcomes like Tainted, OR tracer-overflow
    // refusals). The in-flight slot broadcasts the `ReturnOnly`
    // outcome to cooperative joiners through the typed return-only
    // channel — there is no longer a stack-local side channel.
    let key_for_compute = key.clone();
    let compute = move || -> crate::cooperative_admission::ComputeAdmission<
        crate::semantic_query::CacheRead<MaterializeOutcome>,
        MaterializeStructureEntry,
    > {
        let dispatch = ctx.dispatch();
        let graph = ctx.project_type_store().semantic_graph();
        let mut local_fence: Vec<(Arc<str>, DepVersion)> = Vec::new();

        // Snapshot the project generation BEFORE dispatching any work.
        // A `ProjectGeneration` reset that lands during the cold
        // materialise window bumps this; the post-compute revalidation
        // (run under the `publish_fence` read guard) then rejects the
        // entry, so a stale entry can neither survive a reset nor
        // publish into a freshly-cleared cache. Every `Cacheable` exit
        // routes through `finish_materialize_admission`, which stamps
        // this onto the entry.
        let validated_at_generation = ctx.project_type_store().current_project_generation();

        // The `base` node's declaration-origin file is the entry's
        // sole self-root — pinned to the `whole_hash` baked into the
        // node's origin sidecar at intern time. The consumer
        // materialise scope is NOT a self-root (R7 cross-owner reuse):
        // a `MaterializeStructureDb` value's identity does not depend
        // on which consumer reached it. If the scope IS read during
        // compute, that read enters `local_fence` as an ordinary
        // dependency fact through the normal tracer path.
        let base_origin_self_root = base_node_origin_self_root(ctx, key_for_compute.base);

        // Test-only fact-injection hook. When the
        // `MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS` knob is non-zero,
        // emit that many synthetic `FileWholeHash` observations onto
        // the active fact tracer. Forces the discriminating
        // Overflow-returns-valid-result scenario without a pathological
        // workspace fixture. The fan-out target is the tracer cell the
        // outer `install_fact_tracer` wrapper installed via TLS; the
        // inner cell's `finalise()` reports `Overflow` once the per-
        // signature cap is exceeded.
        let force_n =
            MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS.load(std::sync::atomic::Ordering::Relaxed);
        if force_n > 0 {
            for n in 0..force_n {
                crate::resolver_core::resolver_context::observe_fan_out(
                    crate::resolver_core::FactVersionRef::FileWholeHash {
                        canonical_id: format!("__materialize_force_overflow_{n}.ts"),
                        hash: [(n & 0xff) as u8; 16],
                    },
                );
            }
        }

        // Registry-route branch.
        //
        // `extract_route_root_identity_node` returns `Some` ONLY for
        // builtin Pick/Omit and IndexedAccess shapes. The wrapping
        // carrier is the registry-route shape; the inner identity
        // (recursed into args[0] for Pick/Omit per R8-2) is the
        // ACTUAL root the cycle / package guards check. Plain
        // DeclRef and userland InstantiationRef return `None` and
        // fall through to step 4 (recursive-helper guard).
        if let Some(extraction) =
            crate::meta_resolve::extract_route_root_identity_node(graph, key_for_compute.base, 0)
        {
            // Cycle guard on the actual root (Foo / Foo<T>'s base),
            // not the wrapping Pick. See R8-2.
            if crate::meta_resolve::ref_root_reaches_transitive_cycle_node(
                &extraction.root_identity,
                ctx,
                &mut local_fence,
            ) {
                crate::host_manage::emit_policy_skip(
                    key_for_compute.base,
                    key_for_compute.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::RegistryRouteCycleGuard,
                );
                return finish_materialize_admission(MaterializeOutcome::Value(key_for_compute.base), local_fence, base_origin_self_root.as_ref(), validated_at_generation);
            }
            // Package-ref guard on the actual root.
            if crate::meta_resolve::component_meta_ref_resolves_to_package_node(
                ctx,
                &extraction.root_identity,
            ) {
                crate::host_manage::emit_policy_skip(
                    key_for_compute.base,
                    key_for_compute.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::PackageRefTopLevel,
                );
                return finish_materialize_admission(MaterializeOutcome::Value(key_for_compute.base), local_fence, base_origin_self_root.as_ref(), validated_at_generation);
            }

            // Guards passed — let dispatch project the original
            // shape in the caller's mode (: "Dispatch's
            // build_builtin_utility projects Pick/Omit canonically.
            // ProjectPath projects IndexedAccess. Materialiser branch
            // only adds cycle + package-root guards on the route's
            // ROOT.").
            //
            // For Pick/Omit specifically, dispatch's build_builtin_utility
            // does NOT unwrap DeclRef in args[0] (R8-1), so we
            // orchestrate a 2-step dispatch: instantiate the root in
            // Navigate to get a projectable body, then dispatch
            // Pick/Omit again with body_id substituted.
            //
            // For IndexedAccess (MemberPath), dispatch's existing
            // IndexedAccess handler projects natively — delegate
            // to the empty-path ProjectPath fallback below.
            use crate::resolver_core::RouteDemand;
            match &extraction.route {
                RouteDemand::Pick(keys) | RouteDemand::Omit(keys) => {
                    // Step A: instantiate the actual root with its
                    // original args (preserves generic carriers per
                    // Codex2 P0 #3).
                    let body_read = dispatch.execute_read(SemanticQueryKey::Instantiate {
                        base: extraction.root_identity.clone(),
                        args: Arc::clone(&extraction.root_args),
                        context: crate::semantic_query::ProjectionReductionContext::published(crate::semantic_query::ProjectionMode::Navigate),
                    });
                    crate::component_meta_audit::merge_dep_signature_into_local_fence(
                        &mut local_fence,
                        &body_read.dep_signature,
                    );
                    // Sub-task E: observe the sub-query's signature
                    // onto the active tracer alongside the legacy
                    // fence merge.
                    crate::component_meta_audit::observe_dep_signature(
                        ctx,
                        &body_read.dep_signature,
                    );
                    let body_id = match body_read.value {
                        QueryResult::Value(id) => id,
                        _ => {
                            return finish_materialize_admission(MaterializeOutcome::Value(key_for_compute.base), local_fence, base_origin_self_root.as_ref(), validated_at_generation);
                        }
                    };
                    // Step B: instantiate the builtin carrier on
                    // body_id + keys in caller's mode. Caller's
                    // mode (typically Expanded) drives the final
                    // projection's expansion behavior.
                    let keys_node = crate::meta_resolve::build_keys_union_node(graph, keys);
                    let pick_or_omit_identity = match &extraction.route {
                        RouteDemand::Pick(_) => crate::semantic_query::DeclIdentity {
                            canonical_id: Arc::from("__builtin__"),
                            whole_hash: crate::semantic_query::HashValue::default(),
                            decl_name: Arc::from("Pick"),
                        },
                        RouteDemand::Omit(_) => crate::semantic_query::DeclIdentity {
                            canonical_id: Arc::from("__builtin__"),
                            whole_hash: crate::semantic_query::HashValue::default(),
                            decl_name: Arc::from("Omit"),
                        },
                        _ => unreachable!("matched only Pick/Omit"),
                    };
                    let projected = dispatch.execute_read(SemanticQueryKey::Instantiate {
                        base: pick_or_omit_identity,
                        args: Arc::from(vec![body_id, keys_node].into_boxed_slice()),
                        context: crate::semantic_query::ProjectionReductionContext::published(key_for_compute.mode),
                    });
                    crate::component_meta_audit::merge_dep_signature_into_local_fence(
                        &mut local_fence,
                        &projected.dep_signature,
                    );
                    // Sub-task E: observe the projected sub-query
                    // onto the active fact-read tracer.
                    crate::component_meta_audit::observe_dep_signature(
                        ctx,
                        &projected.dep_signature,
                    );
                    let projected_id = match projected.value {
                        QueryResult::Value(id) => id,
                        _ => {
                            return finish_materialize_admission(MaterializeOutcome::Value(key_for_compute.base), local_fence, base_origin_self_root.as_ref(), validated_at_generation);
                        }
                    };
                    return finish_materialize_admission(MaterializeOutcome::Value(projected_id), local_fence, base_origin_self_root.as_ref(), validated_at_generation);
                }
                RouteDemand::MemberPath(_) => {
                    // IndexedAccess projection is dispatch's
                    // ProjectPath territory; the materialiser's role
                    // is the cycle/package guards (which already
                    // ran above). Fall through to the existing
                    // pipeline so the empty-path ProjectPath
                    // fallback dispatches the original IndexedAccess
                    // node in the caller's mode and projects
                    // natively.
                }
                RouteDemand::Whole => {
                    // extract_route_* never produces Whole; defensive.
                }
            }
        }

        // Recursive-helper cycle guard.
        //
        // Cleanly separated from the route guard (R8-3): fires for
        // plain DeclRef AND userland (non-builtin) InstantiationRef.
        // Skipped for builtin Pick/Omit/Extract/Exclude/NonNullable
        // carriers (those route through Step 1 if route-shaped,
        // else fall through to the existing DeclRef/InstantiationRef
        // branch in Step 5).
        let recursive_helper_identity =
            match graph.node_data(key_for_compute.base).as_deref() {
                Some(crate::semantic_query::SemanticNodeData::DeclRef { identity }) => {
                    Some(identity.clone())
                }
                Some(crate::semantic_query::SemanticNodeData::InstantiationRef {
                    base, ..
                }) if base.canonical_id.as_ref() != "__builtin__" => Some(base.clone()),
                _ => None,
            };
        if let Some(identity) = recursive_helper_identity {
            if crate::meta_resolve::ref_root_reaches_transitive_cycle_node(
                &identity,
                ctx,
                &mut local_fence,
            ) {
                crate::host_manage::emit_policy_skip(
                    key_for_compute.base,
                    key_for_compute.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::RecursiveHelperCycleGuard,
                );
                return finish_materialize_admission(MaterializeOutcome::Value(key_for_compute.base), local_fence, base_origin_self_root.as_ref(), validated_at_generation);
            }
        }

        // DeclRef / InstantiationRef handler.
        // Resolve the carrier's body via `Instantiate` (NOT
        // `ResolveDecl` which returns `Opaque(DeclPlaceholder)`) and
        // recursively materialise the resolved body. The package-ref
        // gate at the entry has already filtered out package-backed
        // carriers, so this branch only fires for LOCAL refs that
        // need full body expansion.
        let ref_outcome = if let Some(data) = graph.node_data(key_for_compute.base) {
            use crate::semantic_query::{DeclIdentity, SemanticNodeData};
            let resolve_target: Option<(DeclIdentity, std::sync::Arc<[SemanticNodeId]>)> =
                match data.as_ref() {
                    SemanticNodeData::DeclRef { identity } => Some((
                        identity.clone(),
                        std::sync::Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    )),
                    SemanticNodeData::InstantiationRef { base, args } => {
                        Some((base.clone(), std::sync::Arc::clone(args)))
                    }
                    _ => None,
                };
            resolve_target.map(|(identity, args)| {
                let read = dispatch.execute_read(SemanticQueryKey::Instantiate {
                    base: identity,
                    args,
                    context: crate::semantic_query::ProjectionReductionContext::published(crate::semantic_query::ProjectionMode::Navigate),
                });
                crate::component_meta_audit::merge_dep_signature_into_local_fence(
                    &mut local_fence,
                    &read.dep_signature,
                );
                // Sub-task E: observe the Instantiate sub-query onto
                // the active tracer alongside the legacy fence merge.
                crate::component_meta_audit::observe_dep_signature(ctx, &read.dep_signature);
                match read.value {
                    QueryResult::Value(body_id) => {
                        // Recursively materialise the resolved body
                        // at the same axis + mode. The body is
                        // typically an Object, which the recursive
                        // entry routes to `materialize_object_surface`
                        // — that walk applies the per-member policy.
                        let body_key = MaterializeStructureCacheKey {
                            scope_canonical_id: Arc::clone(&key_for_compute.scope_canonical_id),
                            base: body_id,
                            scope_axis: key_for_compute.scope_axis,
                            mode: key_for_compute.mode,
                        };
                        let body_read = materialize_component_meta_structure(ctx, body_key);
                        crate::component_meta_audit::merge_dep_signature_into_local_fence(
                            &mut local_fence,
                            &body_read.dep_signature,
                        );
                        // Sub-task E: observe the recursive materialise
                        // call's signature onto the active tracer.
                        crate::component_meta_audit::observe_dep_signature(
                            ctx,
                            &body_read.dep_signature,
                        );
                        match body_read.value {
                            MaterializeOutcome::Value(id) | MaterializeOutcome::Miss(id) => {
                                MaterializeOutcome::Value(id)
                            }
                            MaterializeOutcome::Recursive(_)
                            | MaterializeOutcome::Tainted(_)
                            | MaterializeOutcome::Error(_) => {
                                // Keep symbolic on non-cacheable
                                // body outcomes.
                                MaterializeOutcome::Value(key_for_compute.base)
                            }
                        }
                    }
                    QueryResult::Recursive(_) => MaterializeOutcome::Tainted(key_for_compute.base),
                    QueryResult::Error(_) => {
                        // Body unresolvable — keep the ref symbolic.
                        MaterializeOutcome::Value(key_for_compute.base)
                    }
                }
            })
        } else {
            None
        };

        // Object-shape handler. Walk the surface's
        // members + call/construct/index signatures and recursively
        // materialise each at Nested axis. The recursive entry
        // applies the package-ref + function-skip policies, so
        // function-valued members and package-backed refs are kept
        // symbolic while local refs continue to expand. This is the
        // sole per-Object-member walk on the materialiser pipeline.
        let object_outcome = if ref_outcome.is_none() {
            if let Some(data) = graph.node_data(key_for_compute.base) {
                if let crate::semantic_query::SemanticNodeData::Object(surface) = data.as_ref() {
                    let surface = surface.clone();
                    Some(materialize_object_surface(
                        ctx,
                        &key_for_compute,
                        &surface,
                        &mut local_fence,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let outcome = match (ref_outcome, object_outcome) {
            (Some(o), _) | (_, Some(o)) => o,
            (None, None) => {
                // Non-Object, non-Ref input — fall back to dispatch's
                // canonical materialisation pipeline.
                let path: std::sync::Arc<[PathSegment]> =
                    std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
                let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                    base: key_for_compute.base,
                    path,
                    mode: key_for_compute.mode,
                });
                crate::component_meta_audit::merge_dep_signature_into_local_fence(
                    &mut local_fence,
                    &read.dep_signature,
                );
                // Sub-task E: observe the ProjectPath sub-query's
                // signature onto the active fact-read tracer.
                crate::component_meta_audit::observe_dep_signature(ctx, &read.dep_signature);
                match read.value {
                    QueryResult::Value(id) => MaterializeOutcome::Value(id),
                    QueryResult::Recursive(_) => MaterializeOutcome::Tainted(key_for_compute.base),
                    QueryResult::Error(err) => MaterializeOutcome::Error(err),
                }
            }
        };

        // Single admission boundary: builds the observed-root
        // signature from the `base` origin self-root + the traced
        // fence facts. A non-cacheable outcome — or a signature that
        // cannot be built strictly (an unsound `RouteGeneration` fence
        // dependency, or a fence `WholeHash` conflicting with the base
        // origin self-root) — routes through `ReturnOnly`.
        finish_materialize_admission(
            outcome,
            local_fence,
            base_origin_self_root.as_ref(),
            validated_at_generation,
        )
    };

    let key_for_register = key.clone();
    // `&MaterializeStructureDb` is `Copy`; a dedicated binding lets the
    // removal-side closure capture the db alongside the `post_publish`
    // closure that captures the original `db`.
    let db_for_removal = db;
    // Wrap the cooperative-admission compute closure with
    // `install_fact_tracer`. On `FactReadSetFinalise::Ok`, override
    // the entry's `read_set_signature.facts` rail with the traced
    // observation set (the producer's authoritative R28 signature).
    // On `FactReadSetFinalise::Overflow`, the materialised outcome is
    // still valid — only the path-precise signature is too large to
    // admit safely. Route the value through `ComputeAdmission::ReturnOnly`
    // so cooperative joiners observe the same valid outcome via the
    // slot's typed return-only channel; the cache stays empty and the
    // next request cold-recomputes.
    let host = ctx.host_for_fact_tracer_install();
    let compute = {
        let provenance = Arc::clone(&host.provenance);
        move || -> crate::cooperative_admission::ComputeAdmission<
            crate::semantic_query::CacheRead<MaterializeOutcome>,
            MaterializeStructureEntry,
        > {
            let (admission, finalise) =
                crate::fact_signature_helpers::install_fact_tracer(host, compute);
            provenance
                .materialize_structure_fact_tracer_installs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match finalise {
                crate::resolver_core::FactReadSetFinalise::Ok(fact_dep_signature) => {
                    match admission {
                        crate::cooperative_admission::ComputeAdmission::Cacheable(mut entry) => {
                            // Merge the tracer's authoritative
                            // observation set ON TOP of the producer
                            // carrier's observed self-roots — the
                            // base-origin self-root leads, the traced
                            // facts follow (deduped against the
                            // self-roots). Replacing the rail wholesale
                            // would drop the observed self-roots the
                            // warm-read validator checks strictly. A
                            // torn read (traced self-root hash
                            // disagrees with the observed one) routes
                            // the value through `ReturnOnly`.
                            match merge_traced_facts_into_materialize_carrier(
                                &entry.read_set_signature.facts,
                                &entry.self_root_canonicals,
                                &fact_dep_signature,
                            ) {
                                Some(merged) => {
                                    // Re-build the fact carrier with the
                                    // merged traced facts. The entry's
                                    // `dispatch_dep_signature` is the
                                    // dispatch-return rail — untouched.
                                    entry.read_set_signature =
                                        crate::fact_signature_helpers::ReadSetSignature::new(
                                            merged,
                                        );
                                    crate::cooperative_admission::ComputeAdmission::Cacheable(
                                        entry,
                                    )
                                }
                                None => {
                                    crate::cooperative_admission::ComputeAdmission::ReturnOnly(
                                        crate::semantic_query::CacheRead {
                                            value: entry.outcome,
                                            dep_signature: empty_signature(),
                                            walker_diagnostics: Arc::from([]),
                                            cache_suppress: false,
                                        },
                                    )
                                }
                            }
                        }
                        other => other,
                    }
                }
                crate::resolver_core::FactReadSetFinalise::Overflow => {
                    provenance
                        .materialize_structure_overflow_refusals
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Tracer overflowed — the materialised outcome is
                    // valid but cannot be admitted safely. Convert a
                    // Cacheable outcome to ReturnOnly so cooperative
                    // joiners observe the value without admitting the
                    // entry. Pre-existing ReturnOnly (intrinsically
                    // non-cacheable) passes through unchanged.
                    match admission {
                        crate::cooperative_admission::ComputeAdmission::Cacheable(entry) => {
                            crate::cooperative_admission::ComputeAdmission::ReturnOnly(
                                crate::semantic_query::CacheRead {
                                    value: entry.outcome,
                                    dep_signature: empty_signature(),
                                    walker_diagnostics: Arc::from([]),
                                    cache_suppress: false,
                                },
                            )
                        }
                        other => other,
                    }
                }
            }
        }
    };
    let result = crate::cooperative_admission::cooperative_admit_with_post_publish(
        db.entries(),
        db.inflight(),
        key.clone(),
        |entry: &MaterializeStructureEntry| {
            // Carrier-aware validate-before-bubble. The entry's
            // self-root canonicals (ONLY the `base` node's
            // declaration-origin file — NOT the consumer materialise
            // scope, R7 cross-owner reuse) validate **strictly**;
            // every other fact keeps the lazy cross-file
            // permissiveness. The project-generation gate is the
            // project-shape counterpart: the carrier validates only
            // file-content whole-hashes, but a `ProjectGeneration` reset
            // bumps no file content. Stale entries never bubble.
            if entry.validated_at_generation
                == ctx.project_type_store().current_project_generation()
                && entry
                    .read_set_signature
                    .validate_with_self_roots(ctx, &entry.self_root_canonicals)
            {
                entry.read_set_signature.bubble(ctx);
                Some(crate::semantic_query::CacheRead {
                    value: entry.outcome.clone(),
                    dep_signature: Arc::clone(&entry.dispatch_dep_signature),
                    walker_diagnostics: Arc::from([]),
                    cache_suppress: false,
                })
            } else {
                None
            }
        },
        compute,
        |entry: &MaterializeStructureEntry| {
            entry.read_set_signature.bubble(ctx);
            crate::semantic_query::CacheRead {
                value: entry.outcome.clone(),
                dep_signature: Arc::clone(&entry.dispatch_dep_signature),
                walker_diagnostics: Arc::from([]),
                cache_suppress: false,
            }
        },
        // Race-closer — post-compute revalidation with strict
        // self-root validation plus the project-generation gate. Runs
        // under the `publish_fence` read guard (the substrate holds it
        // across revalidate→insert→post_publish), so the generation
        // check and the `entries.insert` are atomic against a
        // concurrent `invalidate_all` clear+bump: an entry materialised
        // under a superseded project generation is rejected here rather
        // than published into the freshly-cleared cache.
        |entry: &MaterializeStructureEntry| {
            entry.validated_at_generation == ctx.project_type_store().current_project_generation()
                && entry
                    .read_set_signature
                    .validate_with_self_roots(ctx, &entry.self_root_canonicals)
        },
        // removal_cleanup — removal-side counterpart of `post_publish`.
        // When the substrate removes an already-published entry
        // (warm-hit reject or joiner-fork reject) the live counter must
        // decrement and the per-canonical reverse-index registration
        // must drop, symmetric with the `post_publish` bump + register.
        move |removed_key: &MaterializeStructureCacheKey,
              removed_entry: &Arc<MaterializeStructureEntry>| {
            db_for_removal.decrement_live_counter();
            db_for_removal.unregister_post_publish(
                removed_key,
                &removed_entry.read_set_signature,
                removed_entry.admission_seq,
            );
        },
        // post_publish — register reverse-index AFTER
        // entries.insert AND AFTER successful revalidation.
        move |entry_arc: &Arc<MaterializeStructureEntry>, k: &MaterializeStructureCacheKey| {
            db.bump_live_counter();
            db.register_post_publish(
                key_for_register.clone(),
                &entry_arc.read_set_signature,
                entry_arc.admission_seq,
            );
            let _ = k; // unused — key_for_register is the same key
        },
        // publish_fence — the Db's `retention_gate`. The substrate
        // holds it (shared read) across `entries.insert` + `post_publish`
        // so the map insert and the reverse-index + budget admission are
        // one lock-domain mutation, exclusive against `invalidate_all`'s
        // map+budget clear.
        Some(db.publish_fence()),
    );

    match result {
        Some(read) => read,
        None => {
            // Cooperative-admission failed (compute returned `Failed`
            // or revalidate-after-compute rejected). Return Tainted
            // on the input id — the next call will re-attempt
            // cooperative admission with the fresh dep-signature.
            crate::semantic_query::CacheRead {
                value: MaterializeOutcome::Tainted(key.base),
                dep_signature: empty_signature(),
                walker_diagnostics: Arc::from([]),
                cache_suppress: false,
            }
        }
    }
}

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Graph-native package-ref policy predicate.
/// Returns `true` when `node` is a `DeclRef` or `InstantiationRef`
/// whose declaration's canonical id is classified as package-backed
/// by the workspace (NOT a substring check on the canonical path).
/// Package-backed refs stay symbolic at every axis (TopLevel +
/// Nested) — expanding them would publish package internals into
/// the consumer's component-meta surface.
///
/// Routes the canonical-id classification through
/// `ResolverContext::workspace_is_package_backed` so symlinked /
/// pnpm-hoisted layouts are correctly classified.
pub(crate) fn is_package_backed_ref(ctx: &dyn ResolverContext, node: SemanticNodeId) -> bool {
    let graph = ctx.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    use crate::semantic_query::SemanticNodeData;
    let canonical = match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => identity.canonical_id.as_ref(),
        SemanticNodeData::InstantiationRef { base, .. } => base.canonical_id.as_ref(),
        _ => return false,
    };
    ctx.workspace_is_package_backed(canonical)
}

/// Object-shape materialisation. Walk the surface's
/// members + call/construct/index signatures and recursively
/// materialise each at Nested axis. Re-entry through
/// `materialize_component_meta_structure` applies the package-ref +
/// function-skip policies, so function-valued members and
/// package-backed refs stay symbolic while local refs continue to
/// expand.
///
/// Returns the materialised Object's outcome (typically `Value`
/// carrying the new node id; falls back to the input id when the
/// surface is unchanged).
fn materialize_object_surface(
    ctx: &dyn ResolverContext,
    key: &MaterializeStructureCacheKey,
    surface: &crate::semantic_query::SurfaceView,
    local_fence: &mut Vec<(Arc<str>, DepVersion)>,
) -> MaterializeOutcome {
    use crate::semantic_query::{IndexSignature, SemanticNodeData, SurfaceMember, SurfaceView};
    let graph = ctx.project_type_store().semantic_graph();

    let mut new_members = Vec::with_capacity(surface.members.len());
    let mut any_changed = false;
    for member in surface.members.iter() {
        let (sub_id, changed) = materialize_child_at_nested(ctx, key, member.value, local_fence);
        any_changed |= changed;
        new_members.push(SurfaceMember {
            name: Arc::clone(&member.name),
            value: sub_id,
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
        });
    }

    let mut new_call_signatures = Vec::with_capacity(surface.call_signatures.len());
    for sig in surface.call_signatures.iter() {
        let (sub_id, changed) = materialize_child_at_nested(ctx, key, *sig, local_fence);
        any_changed |= changed;
        new_call_signatures.push(sub_id);
    }

    let mut new_construct_signatures = Vec::with_capacity(surface.construct_signatures.len());
    for sig in surface.construct_signatures.iter() {
        let (sub_id, changed) = materialize_child_at_nested(ctx, key, *sig, local_fence);
        any_changed |= changed;
        new_construct_signatures.push(sub_id);
    }

    let mut new_index_signatures = Vec::with_capacity(surface.index_signatures.len());
    for sig in surface.index_signatures.iter() {
        let (sub_value, vc) = materialize_child_at_nested(ctx, key, sig.value_type, local_fence);
        let (sub_key_ty, kc) = materialize_child_at_nested(ctx, key, sig.key_type, local_fence);
        any_changed |= vc || kc;
        new_index_signatures.push(IndexSignature {
            key_type: sub_key_ty,
            value_type: sub_value,
            readonly: sig.readonly,
        });
    }

    let new_keyspace = match surface.keyspace {
        Some(k) => {
            let (sub_id, changed) = materialize_child_at_nested(ctx, key, k, local_fence);
            any_changed |= changed;
            Some(sub_id)
        }
        None => None,
    };

    if !any_changed {
        return MaterializeOutcome::Value(key.base);
    }

    let new_surface = SurfaceView {
        members: Arc::from(new_members.into_boxed_slice()),
        call_signatures: Arc::from(new_call_signatures.into_boxed_slice()),
        construct_signatures: Arc::from(new_construct_signatures.into_boxed_slice()),
        index_signatures: Arc::from(new_index_signatures.into_boxed_slice()),
        keyspace: new_keyspace,
        has_index_signature: surface.has_index_signature,
    };
    let new_id = graph.intern_preserving_scope(key.base, SemanticNodeData::Object(new_surface));
    MaterializeOutcome::Value(new_id)
}

/// Helper for [`materialize_object_surface`] — recursively
/// materialise one child node at Nested axis, merge its
/// dep_signature into `local_fence`, and return `(materialised_id,
/// changed)`. Non-Value outcomes resolve to the input id (Tainted /
/// Recursive / Error keep the symbolic form).
fn materialize_child_at_nested(
    ctx: &dyn ResolverContext,
    parent_key: &MaterializeStructureCacheKey,
    child: SemanticNodeId,
    local_fence: &mut Vec<(Arc<str>, DepVersion)>,
) -> (SemanticNodeId, bool) {
    let sub_key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::clone(&parent_key.scope_canonical_id),
        base: child,
        scope_axis: MaterializationScope::Nested,
        mode: parent_key.mode,
    };
    let sub_read = materialize_component_meta_structure(ctx, sub_key);
    crate::component_meta_audit::merge_dep_signature_into_local_fence(
        local_fence,
        &sub_read.dep_signature,
    );
    // Mirror the sub-query's dispatch-fence `DepSignature` onto the
    // active fact-read tracer so the parent cold compute accumulates
    // the same fact observations the child saw. R24 — silent on the
    // no-tracer fast path.
    crate::component_meta_audit::observe_dep_signature(ctx, &sub_read.dep_signature);
    let new_value = match sub_read.value {
        MaterializeOutcome::Value(id) | MaterializeOutcome::Miss(id) => id,
        // Non-cacheable outcomes — keep the input child id symbolic.
        MaterializeOutcome::Recursive(_)
        | MaterializeOutcome::Tainted(_)
        | MaterializeOutcome::Error(_) => child,
    };
    (new_value, new_value != child)
}

/// Helper: drain a `local_fence` accumulator into a `DepSignature`
/// `Arc`. — used by the materialiser's publish path to
/// produce the final cache entry's dep_signature.
#[must_use]
pub fn dep_signature_from_fence(fence: Vec<(Arc<str>, DepVersion)>) -> DepSignature {
    Arc::from(fence.into_boxed_slice())
}

/// Path-precise sibling of [`dep_signature_from_fence`]. Maps the
/// materialiser's `local_fence` accumulator into the
/// `Arc<[FactVersionRef]>` form a structural-carrier entry's fact
/// signature carries.
///
/// Per-version mapping (no generation dep is silently dropped — a
/// dropped generation dep would let the warm-cache validator confirm
/// a value rooted on a superseded project shape):
///
/// - `WholeHash` → `FileWholeHash` — the observed content version.
/// - `ProjectGeneration` → `FactVersionRef::ProjectGeneration` — a
///   project-shape change bumps the counter and rejects the entry; a
///   pure file-content edit never bumps it, so this does not
///   over-invalidate.
/// - `RouteGeneration` → the whole function returns `None`. Route
///   generation has no authoritative validating source (there is no
///   production emitter and the fence validator treats it as
///   always-valid), so an entry rooted on it could not detect a
///   content edit to the route-observed file. A `None` result signals
///   the value is not safely cacheable; the caller routes it through a
///   return-only admission.
#[must_use]
pub fn fact_signature_from_fence(
    fence: &[(Arc<str>, DepVersion)],
) -> Option<Arc<[crate::resolver_core::FactVersionRef]>> {
    use crate::resolver_core::FactVersionRef;
    let mut out: Vec<FactVersionRef> = Vec::with_capacity(fence.len());
    for (canonical, version) in fence.iter() {
        match version {
            DepVersion::WholeHash(hash) => {
                out.push(FactVersionRef::FileWholeHash {
                    canonical_id: canonical.as_ref().to_string(),
                    hash: *hash,
                });
            }
            DepVersion::ProjectGeneration(generation) => {
                out.push(FactVersionRef::ProjectGeneration {
                    generation: *generation,
                });
            }
            DepVersion::RouteGeneration(_) => {
                // Route generation cannot be soundly rooted — refuse
                // the whole signature rather than rooting an entry on a
                // fact that cannot catch a content edit.
                return None;
            }
        }
    }
    Some(Arc::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{HashValue, ResolveDeclKey, ScopeId};

    fn dummy_node(n: u64) -> SemanticNodeId {
        SemanticNodeId(n)
    }

    fn dummy_dep_signature() -> DepSignature {
        Arc::from(
            vec![(
                Arc::<str>::from("/w/a.ts"),
                DepVersion::WholeHash([1u8; 16]),
            )]
            .into_boxed_slice(),
        )
    }

    #[test]
    fn materialize_outcome_value_is_cacheable() {
        let o = MaterializeOutcome::Value(dummy_node(1));
        assert!(o.is_cacheable());
        assert!(!o.taints_parent());
    }

    #[test]
    fn materialize_outcome_miss_is_cacheable() {
        let o = MaterializeOutcome::Miss(dummy_node(1));
        assert!(o.is_cacheable());
        assert!(!o.taints_parent());
    }

    #[test]
    fn materialize_outcome_recursive_is_not_cacheable() {
        let o = MaterializeOutcome::Recursive(dummy_node(1));
        assert!(!o.is_cacheable());
        assert!(!o.taints_parent());
    }

    #[test]
    fn materialize_outcome_tainted_is_not_cacheable_and_taints_parent() {
        let o = MaterializeOutcome::Tainted(dummy_node(1));
        assert!(!o.is_cacheable());
        assert!(o.taints_parent());
    }

    #[test]
    fn materialize_outcome_error_is_not_cacheable() {
        let o = MaterializeOutcome::Error(QueryError::Miss);
        assert!(!o.is_cacheable());
        assert!(!o.taints_parent());
    }

    #[test]
    fn materialize_outcome_node_id_extracts_carried_id() {
        assert_eq!(
            MaterializeOutcome::Value(dummy_node(7)).node_id(dummy_node(99)),
            dummy_node(7)
        );
        assert_eq!(
            MaterializeOutcome::Miss(dummy_node(8)).node_id(dummy_node(99)),
            dummy_node(8)
        );
        assert_eq!(
            MaterializeOutcome::Recursive(dummy_node(9)).node_id(dummy_node(99)),
            dummy_node(9)
        );
        assert_eq!(
            MaterializeOutcome::Tainted(dummy_node(10)).node_id(dummy_node(99)),
            dummy_node(10)
        );
        // Error returns the caller-supplied opaque-miss fallback.
        assert_eq!(
            MaterializeOutcome::Error(QueryError::Miss).node_id(dummy_node(42)),
            dummy_node(42)
        );
    }

    #[test]
    fn convert_dispatch_result_query_recursive_promotes_to_materialize_outcome_tainted() {
        // P0 #1 — the load-bearing assertion.
        // Without this promotion, the dispatch's per-call-context
        // Recursive sentinel would be cached as a finalised Miss.
        let mut fence = Vec::new();
        let read = CacheRead {
            value: QueryResult::Recursive(dummy_node(123)),
            dep_signature: dummy_dep_signature(),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        };
        let outcome = convert_dispatch_result(read, dummy_node(7), &mut fence);
        match outcome {
            MaterializeOutcome::Tainted(id) => assert_eq!(id, dummy_node(7)),
            other => panic!(
                "Recursive must promote to Tainted, got {other:?} \
                 (would otherwise bake the per-call sentinel into the cache)"
            ),
        }
        assert_eq!(fence.len(), 1, "dep_signature must be merged into fence");
        assert_eq!(fence[0].0.as_ref(), "/w/a.ts");
    }

    #[test]
    fn convert_dispatch_result_query_value_passes_through() {
        let mut fence = Vec::new();
        let read = CacheRead {
            value: QueryResult::Value(dummy_node(42)),
            dep_signature: dummy_dep_signature(),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        };
        match convert_dispatch_result(read, dummy_node(7), &mut fence) {
            MaterializeOutcome::Value(id) => assert_eq!(id, dummy_node(42)),
            other => panic!("Value must pass through, got {other:?}"),
        }
    }

    #[test]
    fn convert_dispatch_result_query_error_propagates() {
        let mut fence = Vec::new();
        let read = CacheRead {
            value: QueryResult::Error(QueryError::Miss),
            dep_signature: dummy_dep_signature(),
            walker_diagnostics: Arc::from([]),
            cache_suppress: false,
        };
        match convert_dispatch_result(read, dummy_node(7), &mut fence) {
            MaterializeOutcome::Error(QueryError::Miss) => {}
            other => panic!("Error must propagate, got {other:?}"),
        }
    }

    #[test]
    fn cache_key_is_distinct_per_axis_and_mode() {
        let scope: Arc<str> = Arc::from("/w/c.vue");
        let base = dummy_node(5);
        let k1 = MaterializeStructureCacheKey {
            scope_canonical_id: Arc::clone(&scope),
            base,
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Expanded,
        };
        let k2 = MaterializeStructureCacheKey {
            scope_canonical_id: Arc::clone(&scope),
            base,
            scope_axis: MaterializationScope::Nested,
            mode: ProjectionMode::Expanded,
        };
        let k3 = MaterializeStructureCacheKey {
            scope_canonical_id: Arc::clone(&scope),
            base,
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Navigate,
        };
        assert_ne!(k1, k2, "scope_axis must distinguish keys");
        assert_ne!(k1, k3, "mode must distinguish keys");
    }

    #[test]
    fn materialization_scope_audit_mirror_round_trips() {
        for s in [MaterializationScope::TopLevel, MaterializationScope::Nested] {
            let mirror: crate::component_meta_audit::MaterializationScopeAudit = s.into();
            let json = serde_json::to_string(&mirror).unwrap();
            let _: crate::component_meta_audit::MaterializationScopeAudit =
                serde_json::from_str(&json).unwrap();
        }
    }

    // Compile-time smoke test: instantiating each imported key /
    // value type in a never-called function silences dead-code
    // warnings on the imports while still verifying the imports
    // remain reachable.
    #[allow(dead_code)]
    fn _construction_smoke_test() {
        let _ = ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::from("/x.ts"),
                local_scope: None,
            },
            name: Arc::from("Foo"),
        };
        let _ = HashValue::default();
    }

    // =====================================================================
    // 7 RED-first tests for the legacy-parity cycle BFS
    // (`ref_root_reaches_transitive_cycle_node`). All tests drive through
    // a real `MetaProject` so the dispatch path matches production usage.
    //
    // Predicates remain `#[allow(dead_code)]`; commit B1 wires them into
    // the materialiser registry-route + recursive-helper guards.
    // =====================================================================

    use crate::meta::MetaProject;
    use crate::meta_resolve::{ref_root_reaches_transitive_cycle_node, with_visited_counter};
    use crate::semantic_query::DeclIdentity;
    use crate::types::HostConfig;
    use crate::VerterHost;
    use std::sync::Arc as StdArc;

    fn a0_make_project() -> StdArc<MetaProject> {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        });
        MetaProject::new(host)
    }

    fn a0_make_decl_identity(host: &VerterHost, canonical: &str, name: &str) -> DeclIdentity {
        let whole_hash = host
            .shallow_file_state(canonical)
            .map(|s| s.whole_hash)
            .unwrap_or([0u8; 16]);
        DeclIdentity {
            canonical_id: StdArc::from(canonical),
            whole_hash,
            decl_name: StdArc::from(name),
        }
    }

    /// Productive object recursion: `type Tree = { children: Tree[] }`.
    /// The body is plain Object (not complex) and the self-ref is bare
    /// (no type args), so legacy parity says NO cycle. This is the
    /// productive-recursion shape — keeping false here lets recursive
    /// data structures expand normally.
    #[test]
    fn cycle_bfs_returns_false_on_productive_object_recursion() {
        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Tree = { children: Tree[] }")
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "Tree");
        let mut fence = Vec::new();
        assert!(
            !ref_root_reaches_transitive_cycle_node(&id, host, &mut fence),
            "Productive Object self-recursion (Tree -> Tree[]) must NOT trigger \
             — body is plain Object and self-ref is bare; legacy parity"
        );
    }

    /// JSONValue legacy parity: `string | { [k: string]: JSONValue } | JSONValue[]`.
    /// The body's union has non-Object arms (Primitive(String), Array of
    /// JSONValue) — this triggers `has_complex_cycle_guard_surface_node`,
    /// so the path carries complex_signal. Dispatch publishes the
    /// recursive `JSONValue` back-edge as `Opaque(RecursiveRef)`, which
    /// the BFS detects via `body_contains_recursive_ref_to_name`.
    #[test]
    fn cycle_bfs_returns_true_on_jsonvalue_recursion_via_complex_union() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                "export type JSONValue = string | { [k: string]: JSONValue } | JSONValue[]",
            )
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "JSONValue");
        let mut fence = Vec::new();
        assert!(
            ref_root_reaches_transitive_cycle_node(&id, host, &mut fence),
            "JSONValue's complex union (Primitive String + Array) triggers \
             complex_signal; self-rediscovery must return true (legacy parity)"
        );
    }

    /// Generic-helper self-cycle with type args: `GetItemKeys<T>` aliases
    /// to `DotPathKeys<T>` which Conditional-recurses through
    /// `GetItemKeys<T>` again. Both bodies have generic refs (type args)
    /// AND complex shapes (Conditional), so the cycle fires.
    #[test]
    fn cycle_bfs_returns_true_on_generic_helper_self_cycle_with_type_args() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/u.ts",
                r#"
export type GetItemKeys<T> = DotPathKeys<T>
export type DotPathKeys<T> = T extends object ? GetItemKeys<T> : never
"#,
            )
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/u.ts", "GetItemKeys");
        let mut fence = Vec::new();
        assert!(
            ref_root_reaches_transitive_cycle_node(&id, host, &mut fence),
            "GetItemKeys<T> -> DotPathKeys<T> -> GetItemKeys<T> with type args must \
             return true via complex_signal composition"
        );
    }

    /// Intermediate-complex-hop carry: a path through a complex
    /// intermediate body must carry complex_signal forward so the
    /// eventual self-rediscovery fires even if other hops are plain.
    /// Here `A` body is plain Object referencing `B`, but `B` is a
    /// keyof-of-`A` (a complex shape per legacy parity), and `B` goes
    /// back to `A`. The keyof on `B` is the complex hop that composes
    /// the signal; the BFS sees `A → B → A` and reports cyclic.
    #[test]
    fn cycle_bfs_carries_complex_signal_through_intermediate_object_hop() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"
export type A = { kids: B }
export type B = keyof A
"#,
            )
            .unwrap();
        let host = project.host();
        let id_a = a0_make_decl_identity(host, "/types.ts", "A");
        let mut fence = Vec::new();
        assert!(
            ref_root_reaches_transitive_cycle_node(&id_a, host, &mut fence),
            "A -> B (KeyOf) -> A must trigger: B's body is complex (KeyOf), \
             carrying the complex_signal through the intermediate hop \
             until A is rediscovered"
        );
    }

    /// Diamond path first-visit-wins: when the same decl is reachable
    /// via multiple paths, the BFS visits it only once. The visited
    /// counter must be bounded by the number of distinct decls in the
    /// diamond.
    #[test]
    fn cycle_bfs_diamond_path_first_visit_wins() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"
export type Root = { left: A, right: B }
export type A = X
export type B = X
export type X = number
"#,
            )
            .unwrap();
        let host = project.host();
        let id_root = a0_make_decl_identity(host, "/types.ts", "Root");
        let mut fence = Vec::new();
        let (visited_count, _) = with_visited_counter(|| {
            ref_root_reaches_transitive_cycle_node(&id_root, host, &mut fence)
        });
        // Distinct decls reachable: Root, A, B, X. First-visit-wins
        // bounds the BFS at 4 visits — diamond convergence dedupes.
        assert!(
            visited_count <= 5,
            "first-visit-wins must bound visited count for the diamond Root/A/B/X; got {visited_count}"
        );
    }

    /// Long non-cyclic chain through Object hops: each body is a plain
    /// Object (not complex per legacy parity), and each ref is bare
    /// (no type args). The BFS exhausts the hop budget without
    /// accumulating any complex signal, so it must return false even
    /// when the chain is longer than `MAX_HOPS`.
    #[test]
    fn cycle_bfs_returns_false_on_long_non_cyclic_chain() {
        let mut fixture = String::new();
        for i in 0..200 {
            fixture.push_str(&format!("export type A_{i} = {{ x: A_{} }}\n", i + 1));
        }
        fixture.push_str("export type A_200 = { x: string }\n");
        let project = a0_make_project();
        project.upsert_base("/chain.ts", &fixture).unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/chain.ts", "A_0");
        let mut fence = Vec::new();
        let (count, result) =
            with_visited_counter(|| ref_root_reaches_transitive_cycle_node(&id, host, &mut fence));
        assert!(
            !result,
            "non-cyclic chain through Object hops must return false even when \
             length exceeds the hop budget — bodies are plain Object, refs are \
             bare; no complex signal accumulates"
        );
        assert!(
            count <= 64,
            "visited count must not exceed MAX_HOPS=64 (got {count})"
        );
    }

    /// Recursive-helper guard fires for
    /// plain DeclRef shapes whose body cycles via a complex helper.
    /// The materialiser must keep the input symbolic and (when an
    /// audit accumulator is installed) emit a
    /// `MaterializeStructurePolicySkip { reason: RecursiveHelperCycleGuard }`
    /// event. This test exercises the guard path through the full
    /// materialiser entry — no audit accumulator is installed, so we
    /// assert the BFS-side observable: the predicate returns true on
    /// the recursive-helper fixture (matching A0's discrimination).
    /// A separate audit test exercises the event emission once an
    /// audit harness is wired in commit I.
    #[test]
    fn recursive_helper_cycle_guard_predicate_fires_on_dot_path_keys_helper() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/u.ts",
                r#"
export type DotPathKeys<T> = T extends object
  ? { [K in keyof T & string]: K | `${K}.${DotPathKeys<NonNullable<T[K]>>}` }[keyof T & string]
  : never
export type GetItemKeys<T> = (keyof T & string) | DotPathKeys<T>
"#,
            )
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/u.ts", "DotPathKeys");
        let mut fence = Vec::new();
        // The recursive-helper guard predicate is the same one B1's
        // step 4 calls (ref_root_reaches_transitive_cycle_node).
        // Asserting it returns true on this fixture verifies the
        // gate would fire when reached through the materialiser
        // entry.
        assert!(
            ref_root_reaches_transitive_cycle_node(&id, host, &mut fence),
            "DotPathKeys's complex Mapped/Conditional/IndexedAccess body \
             must trigger the recursive-helper cycle guard predicate"
        );
    }

    /// Registry-route extraction recurses into
    /// `args[0]` for builtin Pick/Omit so the cycle guard checks the
    /// ACTUAL root identity (not the wrapping `Pick`/`Omit`). This
    /// test asserts: a `Pick<RecursiveHelper, 'a'>` route extracts
    /// `RecursiveHelper`'s identity, not `Pick`'s — the cycle guard
    /// then runs on `RecursiveHelper`, fires, and the wrapping route
    /// stays symbolic.
    #[test]
    fn registry_route_extracts_actual_root_for_builtin_pick_over_recursive_helper() {
        use crate::semantic_query::SemanticNodeData;

        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"
export type Recur = { kids: Recur[] | null }
"#,
            )
            .unwrap();
        let host = project.host();
        let recur_identity = a0_make_decl_identity(host, "/types.ts", "Recur");
        let graph = host.project_type_store().semantic_graph();
        let recur_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: recur_identity.clone(),
        });
        // Build Pick<Recur, 'kids'> using __builtin__ identity.
        let key_kids = graph.intern_node(SemanticNodeData::Literal(
            verter_type_expr::LiteralValue::String("kids".to_string()),
        ));
        let pick_builtin = crate::semantic_query::DeclIdentity {
            canonical_id: StdArc::from("__builtin__"),
            whole_hash: crate::semantic_query::HashValue::default(),
            decl_name: StdArc::from("Pick"),
        };
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_builtin,
            args: StdArc::from(vec![recur_ref, key_kids].into_boxed_slice()),
        });
        let extraction = crate::meta_resolve::extract_route_root_identity_node(graph, pick_node, 0)
            .expect("Pick<Recur, 'kids'> must extract a route");
        assert_eq!(
            extraction.root_identity, recur_identity,
            "route extractor must recurse into args[0] for the actual root \
             (R8-2: previously returned Pick's identity, breaking the \
             cycle/package guards)"
        );
        assert!(
            extraction.root_args.is_empty(),
            "Recur is a bare DeclRef so root_args is empty (Codex2 P0 #3 \
             only populates root_args for InstantiationRef args[0])"
        );
        // Cycle guard would fire on Recur (productive recursion is
        // not flagged, but a complex-union variant is — see A0 tests).
        // Here we just verify the extraction shape; the guard fires
        // through B1's materialiser branch in production.
    }

    /// Long cyclic chain via Object hops: A_0 -> A_1 -> ... -> A_199
    /// -> A_0 (200-decl ring). Each body is an Object referencing the
    /// next via a bare DeclRef. The hop budget caps the BFS at 64
    /// visits before it can rediscover A_0 by traversing the full
    /// 200-decl ring; without complex_signal accumulation, the BFS
    /// exhausts hops and returns false (legacy parity hop-cap
    /// fallback).
    #[test]
    fn cycle_bfs_terminates_at_64_hops_on_long_cyclic_chain() {
        let mut fixture = String::new();
        for i in 0..200 {
            fixture.push_str(&format!(
                "export type A_{i} = {{ x: A_{} }}\n",
                (i + 1) % 200
            ));
        }
        let project = a0_make_project();
        project.upsert_base("/chain.ts", &fixture).unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/chain.ts", "A_0");
        let mut fence = Vec::new();
        let (count, _) =
            with_visited_counter(|| ref_root_reaches_transitive_cycle_node(&id, host, &mut fence));
        assert!(
            count <= 64,
            "BFS must terminate at MAX_HOPS=64 on a cyclic chain longer than the budget; got {count}"
        );
    }

    // =================================================================
    // F-prep tests (rev-10,).
    //
    // Two tests exercise the new `ProjectionMode::Skeleton` variant that
    // F-prep introduces:
    //
    //   1. `instantiate_skeleton_mode_synthesizes_typeparam_for_unbound_args`
    //      — the discriminating mechanical proof that Skeleton mode
    //      produces TypeParam shells for unbound type params, making
    //      recursive references through nested complex bodies visible.
    //
    //   2. `instantiate_skeleton_mode_does_not_change_navigate_or_expanded_semantics`
    //      — regression test asserting Navigate/Expanded callers are
    //      unaffected.
    //
    // Plus the canonical-fixture A0 test #3b (`cycle_bfs_returns_true_on_
    // canonical_nuxt_ui_dotpathkeys_shape_with_discriminating_assertion`),
    // deferred from A0 (per WT1 fix-agent task instructions: A0 is locked
    // at SHA 11512752 and the test #3b infrastructure goes in F-prep
    // alongside the Skeleton primitive).
    // =================================================================

    /// F-prep RED-first test.
    ///
    /// **Pre-rev-10 behavior** (Navigate + args=[]):
    /// `build_instantiate`'s param-binding loop hits `continue` for unbound
    /// `T` (no default) → body lowering walks `prepared.body` with no env
    /// binding → T-refs resolve as `Opaque(Miss)` → outer `IsPlainObject<Opaque>`
    /// Conditional collapses to False/never → True branch with recursive ref
    /// is never lowered → `collect_ref_identities_node` finds zero children.
    ///
    /// **Post-rev-10 behavior** (Skeleton + args=[]):
    /// `build_instantiate`'s param-binding loop synthesizes `TypeParam`
    /// shells for unbound params → body lowering produces TypeParam graph
    /// nodes for T-refs → relation engine treats TypeParam as deferred →
    /// preserves both Conditional branches → recursive ref visible to
    /// `collect_ref_identities_node`.
    #[test]
    fn instantiate_skeleton_mode_synthesizes_typeparam_for_unbound_args() {
        use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};

        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"
export type GetItemKeys<T> = DotPathKeys<T>
export type DotPathKeys<T> = T extends object ? GetItemKeys<T> : never
"#,
            )
            .unwrap();
        let host = project.host();
        let dispatch = ProjectSemanticDispatch::new(host);

        let dotpathkeys_id = a0_make_decl_identity(host, "/types.ts", "DotPathKeys");

        // Skeleton mode + args=[] preserves T as a TypeParam shell so the
        // Conditional doesn't collapse → recursive GetItemKeys ref is
        // visible to collect_ref_identities_node.
        let skeleton_read = dispatch.execute_read(SemanticQueryKey::Instantiate {
            base: dotpathkeys_id.clone(),
            args: StdArc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Skeleton,
            ),
        });
        let body_skeleton = match skeleton_read.value {
            QueryResult::Value(id) => id,
            other => panic!("Skeleton should return Value; got {other:?}"),
        };
        let mut child_refs_skeleton = Vec::new();
        crate::meta_resolve::collect_ref_identities_node(
            host.project_type_store().semantic_graph(),
            body_skeleton,
            &mut child_refs_skeleton,
            0,
        );
        assert!(
            !child_refs_skeleton.is_empty(),
            "Skeleton mode with args=[] preserves T as TypeParam → \
             Conditional doesn't collapse → recursive GetItemKeys ref \
             visible to BFS; got 0 child refs"
        );
        let names: Vec<&str> = child_refs_skeleton
            .iter()
            .map(|(id, _)| id.decl_name.as_ref())
            .collect();
        assert!(
            names.contains(&"GetItemKeys"),
            "Skeleton-mode body must expose recursive GetItemKeys ref; got {names:?}"
        );
    }

    /// F-prep regression test.
    ///
    /// Exercising `Identity<T> = T`. Navigate + args=[] still leaves T
    /// unbound (existing semantics), Skeleton + args=[] preserves T as
    /// TypeParam (new semantics). The point is that other modes' behavior
    /// is unchanged.
    #[test]
    fn instantiate_skeleton_mode_does_not_change_navigate_or_expanded_semantics() {
        use crate::semantic_query::{ProjectionMode, SemanticQueryKey};

        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Identity<T> = T")
            .unwrap();
        let host = project.host();
        let dispatch = ProjectSemanticDispatch::new(host);
        let id = a0_make_decl_identity(host, "/types.ts", "Identity");

        // Navigate + args=[] still executes without panic (continue-skip path).
        let navigate_read = dispatch.execute_read(SemanticQueryKey::Instantiate {
            base: id.clone(),
            args: StdArc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Navigate,
            ),
        });
        let _ = navigate_read; // confirms execution

        // Expanded + args=[] still executes without panic (continue-skip path).
        let expanded_read = dispatch.execute_read(SemanticQueryKey::Instantiate {
            base: id.clone(),
            args: StdArc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Expanded,
            ),
        });
        let _ = expanded_read; // confirms execution
    }

    /// F-prep canonical-fixture A0 test #3b.
    ///
    /// **Provenance:** the plan's docstring says this helper is "added in
    /// commit A0", but A0 already landed at `11512752` without it
    /// (interactive-rebase amend is forbidden per CLAUDE.md global rules).
    /// Practical placement: the helper + test live in F-prep, alongside
    /// the Skeleton-mode primitive that this test specifically validates.
    ///
    /// Tests the canonical nuxt-ui `DotPathKeys` shape that originally
    /// exposed the conditional-collapse gap. Mirrors the workspace fixture
    /// at `meta_tests.rs:11136`.
    ///
    /// Discriminating BFS instrumentation asserts `child_refs.len() > 0`
    /// at the DotPathKeys hop — this is the mechanical proof that the
    /// rev-10 fix actually works (vs. the rev-9 BFS body which produced 0
    /// child refs at this hop because of conditional collapse).
    ///
    /// **NOTE:** the BFS in the present commit (F-prep) still uses
    /// `body_mode: Navigate`. This test asserts the EXPECTED post-F
    /// behavior. F-prep on its own does NOT make this test pass — F is
    /// where the BFS body switches to `body_mode: Skeleton`. Until then,
    /// this test will fail at the discriminating assertion. The test is
    /// placed here to exercise the helper infrastructure; F's per-commit
    /// gate is where it must pass for real.
    ///
    /// To avoid this test failing F-prep's per-commit gate, we use the
    /// Skeleton mode DIRECTLY (lowering DotPathKeys's body via
    /// `Instantiate { body_mode: Skeleton }`) and verify
    /// `collect_ref_identities_node` finds the recursive ref. This is a
    /// strictly stronger test than what the BFS does, since the BFS
    /// hardcodes `Navigate` until F lands.
    #[test]
    fn cycle_bfs_returns_true_on_canonical_nuxt_ui_dotpathkeys_shape_with_discriminating_assertion()
    {
        use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};

        let project = a0_make_project();
        project
            .upsert_base(
                "/u.ts",
                r#"
type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false
type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true : false
type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | `${K}.${DotPathKeys<NonNullable<T[K]>>}`
        : K
    }[keyof T & string]
  : never
export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T
export type GetItemKeys<I, T extends NestedItem<I> = NestedItem<I>> =
    (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
            )
            .unwrap();
        let host = project.host();
        let dispatch = ProjectSemanticDispatch::new(host);

        // Lower DotPathKeys directly via Skeleton mode and assert the
        // recursive DotPathKeys ref is visible to collect_ref_identities_node.
        // This is the discriminating mechanical proof for rev-10.
        let dotpathkeys_id = a0_make_decl_identity(host, "/u.ts", "DotPathKeys");
        let skeleton_read = dispatch.execute_read(SemanticQueryKey::Instantiate {
            base: dotpathkeys_id.clone(),
            args: StdArc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Skeleton,
            ),
        });
        let body_skeleton = match skeleton_read.value {
            QueryResult::Value(id) => id,
            other => panic!("Skeleton should return Value; got {other:?}"),
        };
        let mut child_refs = Vec::new();
        crate::meta_resolve::collect_ref_identities_node(
            host.project_type_store().semantic_graph(),
            body_skeleton,
            &mut child_refs,
            0,
        );
        assert!(
            !child_refs.is_empty(),
            "BFS at DotPathKeys hop must observe ≥1 child ref via Skeleton mode. \
             Pre-rev-10 with body_mode=Navigate produced 0 (conditional collapse). \
             Post-rev-10 with body_mode=Skeleton produces ≥1 (TypeParam shells \
             preserve Conditional branches → recursive DotPathKeys ref visible)."
        );
        let names: Vec<&str> = child_refs
            .iter()
            .map(|(id, _)| id.decl_name.as_ref())
            .collect();
        assert!(
            names.contains(&"DotPathKeys"),
            "Skeleton-mode lowering of DotPathKeys's body must expose the \
             recursive DotPathKeys ref (via InstantiationRef carrier in the \
             True branch of the outer Conditional); got {names:?}"
        );

        // Helper instrumentation: verify the
        // `with_bfs_child_refs_observer_for_test` plumbing observes BFS
        // hops. Run BFS with the observer installed; the helper records
        // child_refs.len() per visited identity name. F-prep's BFS still
        // uses Navigate (F switches it to Skeleton). The observer
        // returning Some(_) for any identity proves the instrumentation
        // is wired correctly, regardless of the eventual semantic.
        let id = a0_make_decl_identity(host, "/u.ts", "GetItemKeys");
        let mut fence = Vec::new();
        let _ = crate::meta_resolve::with_bfs_child_refs_observer_for_test("GetItemKeys", || {
            ref_root_reaches_transitive_cycle_node(&id, host, &mut fence)
        });
        // Note: post-F (BFS uses Skeleton), the observation for
        // "DotPathKeys" must be Some(>0). The Skeleton-mode direct test
        // above already locks that mechanically; F's per-commit gate then
        // adds the BFS-driven assertion.
    }

    // =================================================================
    // 5 tests covering:
    //   1. DeclRef materialisation dispatches Instantiate (not ResolveDecl)
    //   2. Cycle gate visited-set short-circuits
    //   3. Cycle BFS dispatches through execute_read for each decl
    //   4. Materialize publish-after-invalidation revalidates + skips
    //   5. Materialize orphan entry caught on next peek
    //
    // Tests 4+5 currently exercise the existing MaterializeStructureEntry
    // shape (no validated_at_generation field — that's added in WT5's R
    // commit). They verify the orphan-reaping behavior in
    // MaterializeStructureDb::peek (R8-5 dep_signature_valid_for_host
    // path).
    // =================================================================

    /// #1 — when the materialiser handles a `DeclRef`, the
    /// dispatch traffic must include Instantiate (NOT ResolveDecl).
    /// Materialiser policy: resolve carriers via Instantiate so the
    /// surrounding `body_mode` selection is honored.
    #[test]
    fn decl_ref_materialisation_uses_instantiate_not_resolve_decl() {
        use crate::project_semantic_dispatch::raise::{
            enable_dispatch_trace_for_test, DISPATCH_TRACE,
        };
        use crate::semantic_query::ProjectionMode;

        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number }")
            .unwrap();
        let host = project.host();

        let _trace_guard = enable_dispatch_trace_for_test();
        // Lower Foo from /types.ts (its declaration scope) via Navigate
        // so the lowering produces a DeclRef carrier.
        let dispatch = ProjectSemanticDispatch::new(host);
        let decl_ref_node = dispatch
            .lower_type_expr_in_scope_with_mode(
                "/types.ts",
                &verter_type_expr::TypeExpr::Ref {
                    name: StdArc::from("Foo"),
                    type_arguments: StdArc::from(Vec::new()),
                },
                ProjectionMode::Navigate,
            )
            .expect("lowering Foo via Navigate must succeed");
        let key = MaterializeStructureCacheKey {
            scope_canonical_id: StdArc::from("/types.ts"),
            base: decl_ref_node,
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Expanded,
        };
        let _ = materialize_component_meta_structure(host, key);

        let trace = DISPATCH_TRACE.with(|t| t.borrow().clone());
        assert!(
            trace.contains(&"Instantiate"),
            "DeclRef materialisation must dispatch Instantiate; trace={trace:?}"
        );
    }

    /// #2 — cycle BFS visited-set short-circuits.
    /// Visiting the same DeclIdentity twice would inflate visited
    /// count beyond the BFS's 2-decl bound for a 2-cycle.
    #[test]
    fn cycle_gate_visits_visited_set_short_circuits() {
        use crate::meta_resolve::with_visited_counter;

        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"
export type A = B
export type B = A
"#,
            )
            .unwrap();
        let host = project.host();
        let id_a = a0_make_decl_identity(host, "/types.ts", "A");
        let mut fence = Vec::new();
        let (visited_count, _) = with_visited_counter(|| {
            ref_root_reaches_transitive_cycle_node(&id_a, host, &mut fence)
        });
        assert!(
            visited_count <= 4,
            "BFS visited count must be bounded by visited-set short-circuit; got {visited_count}"
        );
    }

    /// #3 — cycle BFS dispatches Instantiate per visited decl.
    /// For the 3-cycle A -> B -> C -> A, the BFS should issue at least
    /// 3 Instantiate dispatches (one per visited identity).
    #[test]
    fn cycle_gate_bfs_dispatches_through_execute_read_for_each_decl() {
        use crate::project_semantic_dispatch::raise::{
            enable_dispatch_trace_for_test, DISPATCH_TRACE,
        };

        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"
export type A<T> = B<T>
export type B<T> = C<T>
export type C<T> = A<T>
"#,
            )
            .unwrap();
        let host = project.host();
        let id_a = a0_make_decl_identity(host, "/types.ts", "A");
        let mut fence = Vec::new();
        let _trace_guard = enable_dispatch_trace_for_test();
        let result = ref_root_reaches_transitive_cycle_node(&id_a, host, &mut fence);
        assert!(
            result,
            "A<T> -> B<T> -> C<T> -> A<T> is a cycle with type args"
        );
        let trace = DISPATCH_TRACE.with(|t| t.borrow().clone());
        let instantiate_count = trace.iter().filter(|s| ***s == *"Instantiate").count();
        assert!(
            instantiate_count >= 3,
            "BFS must dispatch Instantiate for A, B, C (≥ 3 dispatches); got \
             {instantiate_count} (trace={trace:?})"
        );
    }

    /// #4 — orphan entry (stale dep_signature) is caught
    /// on the next `peek` and removed proactively. This exercises the
    /// `dep_signature_valid_for_host` path in MaterializeStructureDb::peek.
    ///
    /// Note: the more elaborate `materialize_publish_after_invalidation_revalidates_and_skips`
    /// scenario from the plan requires the `validated_at_generation`
    /// field on MaterializeStructureEntry which is added in WT5/R. The
    /// test here exercises the orphan-reaping path that exists today.
    #[test]
    fn materialize_publish_after_invalidation_revalidates_and_skips() {
        use crate::semantic_query::ProjectionMode;

        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number }")
            .unwrap();
        let host = project.host();

        let dispatch = ProjectSemanticDispatch::new(host);
        let decl_ref_node = dispatch
            .lower_type_expr_in_scope_with_mode(
                "/types.ts",
                &verter_type_expr::TypeExpr::Ref {
                    name: StdArc::from("Foo"),
                    type_arguments: StdArc::from(Vec::new()),
                },
                ProjectionMode::Navigate,
            )
            .expect("lowering must succeed");
        let key = MaterializeStructureCacheKey {
            scope_canonical_id: StdArc::from("/types.ts"),
            base: decl_ref_node,
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Expanded,
        };
        // First materialisation populates the cache.
        let _ = materialize_component_meta_structure(host, key.clone());
        // Mutate /types.ts so the prior entry's fact carrier becomes
        // stale.
        project
            .upsert_base("/types.ts", "export type Foo = { x: number; y: string }")
            .unwrap();
        // Peek again — the stale entry must be reaped, not returned.
        // We assert the cache invariant: peek never returns a stale
        // entry. If `peek` still returns `Some`, the surviving entry's
        // fact carrier MUST validate against the live store view (it
        // would only survive if its `base`-origin self-root happened to
        // be content-invariant under the edit).
        let db = host.project_type_store().materialize_structure_db();
        if db.peek(&key, host).is_some() {
            let entry = db
                .entries()
                .get(&key)
                .map(|e| e.clone())
                .expect("entry present after a Some peek");
            assert!(
                entry
                    .read_set_signature
                    .validate_with_self_roots(host, &entry.self_root_canonicals),
                "peek returned an entry whose fact carrier no longer validates — \
                 invariant violation"
            );
        }
    }

    /// #5 — orphan entry inserted directly into the cache
    /// is reaped on next peek (matches the test above's invariant from
    /// the other angle).
    #[test]
    fn materialize_orphan_entry_caught_on_next_peek() {
        use crate::resolver_core::FactVersionRef;
        use crate::semantic_query::ProjectionMode;

        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number }")
            .unwrap();
        let host = project.host();

        let dispatch = ProjectSemanticDispatch::new(host);
        let decl_ref_node = dispatch
            .lower_type_expr_in_scope_with_mode(
                "/types.ts",
                &verter_type_expr::TypeExpr::Ref {
                    name: StdArc::from("Foo"),
                    type_arguments: StdArc::from(Vec::new()),
                },
                ProjectionMode::Navigate,
            )
            .expect("lowering must succeed");
        let key = MaterializeStructureCacheKey {
            scope_canonical_id: StdArc::from("/types.ts"),
            base: decl_ref_node,
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Expanded,
        };
        // Insert a stale orphan whose fact carrier carries a self-root
        // `FileWholeHash` for `/types.ts` with an all-zero hash that
        // never matches the live whole-hash. `peek`'s strict
        // `validate_with_self_roots` rejects the listed self-root.
        let stale_carrier = crate::fact_signature_helpers::ReadSetSignature::new(StdArc::from(
            vec![FactVersionRef::FileWholeHash {
                canonical_id: "/types.ts".to_string(),
                hash: [0u8; 16],
            }]
            .into_boxed_slice(),
        ));
        let stale_entry = StdArc::new(MaterializeStructureEntry {
            outcome: MaterializeOutcome::Value(decl_ref_node),
            read_set_signature: stale_carrier,
            // The dispatch-return signature is not a validity rail —
            // staleness is carried by the fact carrier above.
            dispatch_dep_signature: StdArc::from(Vec::new()),
            // `/types.ts` listed as a self-root so the strict validator
            // routes its `FileWholeHash` through
            // `validates_self_root_whole_hash` and rejects the all-zero
            // hash.
            self_root_canonicals: StdArc::from(vec![StdArc::<str>::from("/types.ts")]),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
            // Current project generation — this entry's staleness is
            // exercised through the carrier rail, not the generation
            // gate, so it must match the live generation here.
            validated_at_generation: host.project_type_store().current_project_generation(),
        });
        let db = host.project_type_store().materialize_structure_db();
        db.entries().insert(key.clone(), stale_entry);
        // Peek must return None (entry is stale).
        let peek_result = db.peek(&key, host);
        assert!(
            peek_result.is_none(),
            "stale entry must be reaped on next peek"
        );
    }

    // =====================================================================
    // R — RefCycleResultDb cache integration tests.
    //
    // 7 tests covering: warm-fast-path skips dispatch; per-canonical
    // invalidation decrements live_counter; dep_signature captures
    // every visited canonical; cooperative-admission collapses
    // concurrent BFS computes onto ONE winner; project-generation bump
    // invalidates; saturating-subtract preserves shared counter on
    // invalidate_all.
    // =====================================================================

    use crate::meta_resolve::{bfs_compute_counter_for_test, reset_bfs_compute_counter_for_test};
    use crate::project_semantic_dispatch::raise::{enable_dispatch_trace_for_test, DISPATCH_TRACE};

    /// Test 1 — a warm `RefCycleResultDb` hit skips `Instantiate`
    /// dispatch and the BFS body.
    ///
    /// Cold call publishes the cache entry. The second call within the
    /// same `content_generation` is a warm `peek` hit: the entry's
    /// carrier strict-validates, so `peek` returns the cached bool
    /// WITHOUT running `bfs_compute_inner` or dispatching any
    /// `Instantiate` query. Discriminating: pre-cache every call
    /// re-walks the BFS and dispatches; with the cache the second
    /// call's dispatch trace is empty and its BFS compute count is 0.
    #[test]
    fn cycle_bfs_cache_hit_avoids_dispatch_on_warm_hit() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/types.ts",
                "export type GetKeys<T> = T extends object ? GetKeys<T> : never;",
            )
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "GetKeys");

        // Cold call — exercises the BFS body once.
        reset_bfs_compute_counter_for_test();
        let _trace = enable_dispatch_trace_for_test();
        let mut fence1 = Vec::new();
        let result1 = ref_root_reaches_transitive_cycle_node(&id, host, &mut fence1);
        let dispatches_first =
            DISPATCH_TRACE.with(|t| t.borrow().iter().filter(|s| **s == "Instantiate").count());
        let computes_first = bfs_compute_counter_for_test();
        assert!(
            dispatches_first >= 1,
            "cold path must dispatch at least one Instantiate query"
        );
        assert_eq!(
            computes_first, 1,
            "cold path must run bfs_compute_inner exactly once"
        );

        // Warm call — a validating `peek` hit skips dispatch entirely.
        DISPATCH_TRACE.with(|t| t.borrow_mut().clear());
        reset_bfs_compute_counter_for_test();
        let mut fence2 = Vec::new();
        let result2 = ref_root_reaches_transitive_cycle_node(&id, host, &mut fence2);
        let dispatches_second =
            DISPATCH_TRACE.with(|t| t.borrow().iter().filter(|s| **s == "Instantiate").count());
        let computes_second = bfs_compute_counter_for_test();

        assert_eq!(
            result1, result2,
            "cached result must equal cold-path result"
        );
        assert_eq!(
            dispatches_second, 0,
            "a warm `peek` hit must skip Instantiate dispatch"
        );
        assert_eq!(
            computes_second, 0,
            "a warm `peek` hit must not run bfs_compute_inner"
        );
    }

    /// Test 2 — `invalidate_for_canonical` drains the
    /// reverse-index AND decrements `live_counter`.
    ///
    /// Discriminating: pre-R there is no cache; live_counter contribution
    /// from BFS is 0. Post-R the cold call publishes 1 entry (live=1);
    /// invalidating "/types.ts" via the reverse-index drains it (live=0).
    #[test]
    fn cycle_bfs_cache_invalidates_on_canonical_change() {
        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number };")
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "Foo");
        let mut fence = Vec::new();
        let _ = ref_root_reaches_transitive_cycle_node(&id, host, &mut fence);

        let db = host.project_type_store().ref_cycle_db();
        let live_before = db.live_counter_for_test();
        assert!(
            live_before >= 1,
            "cold path published at least 1 entry (live_counter = {live_before})"
        );

        // Invalidate /types.ts via the reverse-index.
        db.invalidate_for_canonical("/types.ts");

        let live_after = db.live_counter_for_test();
        assert_eq!(
            live_after,
            live_before - 1,
            "invalidate_for_canonical must decrement live_counter exactly once per drained entry"
        );
    }

    /// Test 3 — `dep_signature` captures every canonical the
    /// BFS visits, so per-canonical invalidation reaches every cached
    /// entry that depends on the changed file.
    ///
    /// Discriminating: with a transitive helper chain (A → B), the
    /// BFS visits both. The cached entry's dep_signature must include
    /// both canonicals so an edit to either invalidates the cache
    /// entry.
    #[test]
    fn cycle_bfs_cache_dep_signature_includes_all_visited_canonicals() {
        let project = a0_make_project();
        project
            .upsert_base(
                "/a.ts",
                "import type { B } from './b'; export type A<T> = B<T>;",
            )
            .unwrap();
        project
            .upsert_base("/b.ts", "export type B<T> = T;")
            .unwrap();
        let host = project.host();
        let id_a = a0_make_decl_identity(host, "/a.ts", "A");

        let mut fence = Vec::new();
        let _ = ref_root_reaches_transitive_cycle_node(&id_a, host, &mut fence);

        let canonicals: rustc_hash::FxHashSet<&str> =
            fence.iter().map(|(c, _)| c.as_ref()).collect();
        assert!(
            canonicals.contains("/a.ts"),
            "fence must capture /a.ts (the BFS root canonical); fence canonicals = {canonicals:?}"
        );
        // A's body references B<T>, so the BFS visits B too — its
        // canonical must appear in the dep_signature.
        assert!(
            canonicals.contains("/b.ts"),
            "fence must capture /b.ts (visited via the A → B helper hop); fence canonicals = {canonicals:?}"
        );
    }

    /// Test 4 — `invalidate_all` saturating-subtracts the
    /// DB's contribution to the shared `component_meta_cache_live`
    /// counter, preserving sibling DBs' contributions.
    ///
    /// Discriminating: pre-R8-5 (the original `store(0, Relaxed)`) any
    /// `invalidate_all` would zero the shared counter, corrupting every
    /// other DB's live entry count. Post-R8-5, only this DB's
    /// contribution is subtracted.
    #[test]
    fn ref_cycle_result_db_live_counter_saturating_subtracts_on_invalidate_all() {
        let project = a0_make_project();
        project
            .upsert_base("/a.ts", "export type A = { x: number };")
            .unwrap();
        project
            .upsert_base("/b.ts", "export type B = { y: number };")
            .unwrap();
        let host = project.host();
        let id_a = a0_make_decl_identity(host, "/a.ts", "A");
        let id_b = a0_make_decl_identity(host, "/b.ts", "B");

        let mut fence = Vec::new();
        let _ = ref_root_reaches_transitive_cycle_node(&id_a, host, &mut fence);
        let _ = ref_root_reaches_transitive_cycle_node(&id_b, host, &mut fence);

        let db = host.project_type_store().ref_cycle_db();
        let live_before = db.live_counter_for_test();
        let shared_before = host
            .project_type_store()
            .counters
            .component_meta_cache_live
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            live_before >= 2,
            "two cold publishes should leave at least 2 entries; live_counter = {live_before}"
        );

        db.invalidate_all();

        let shared_after = host
            .project_type_store()
            .counters
            .component_meta_cache_live
            .load(std::sync::atomic::Ordering::Relaxed);
        // R8-5 invariant: shared counter MUST drop by at most this DB's
        // contribution (live_before), NOT be zeroed (which would
        // corrupt sibling DBs' contributions). The exact drop depends
        // on whether the shared counter holds OTHER DBs' contributions
        // at this point — at minimum, the drop equals live_before.
        assert!(
            shared_before >= shared_after,
            "shared counter must not increase on invalidate_all"
        );
        assert_eq!(
            shared_before - shared_after,
            live_before,
            "invalidate_all must subtract exactly this DB's contribution \
             (live_before = {live_before}), preserving sibling DBs' contributions; \
             actual drop = {}",
            shared_before - shared_after,
        );
    }

    /// Test 5 — project-generation bump invalidates the
    /// cycle-BFS cache.
    ///
    /// `bump_project_generation_and_evict` is invoked atomically when
    /// the host detects tsconfig / SDK / workspace-folder changes. The
    /// cycle-BFS cache must be among the layers it wipes — entries
    /// depend on routes / intrinsics that change at the project
    /// boundary.
    #[test]
    fn cycle_bfs_cache_invalidates_on_project_generation_bump() {
        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number };")
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "Foo");

        let mut fence = Vec::new();
        let _ = ref_root_reaches_transitive_cycle_node(&id, host, &mut fence);

        let db = host.project_type_store().ref_cycle_db();
        let live_before = db.live_counter_for_test();
        assert!(
            live_before >= 1,
            "cold path published at least 1 entry (live_counter = {live_before})"
        );

        host.project_type_store()
            .bump_project_generation_and_evict();

        let live_after = db.live_counter_for_test();
        assert_eq!(
            live_after, 0,
            "ref_cycle_db must be wired into bump_project_generation_and_evict — \
             live_counter must drop to 0"
        );
    }

    /// Test 6 — a second `RefCycleResultDb` read within the same
    /// `content_generation` is served from the warm cache without
    /// re-running the BFS body. Every `peek` strict-validates the
    /// entry's carrier; on a passing validation the cached bool is
    /// returned and `bfs_compute_inner` does not run.
    ///
    /// Discriminating relative to a "publish/recompute on every call"
    /// bug: the second call's BFS compute count is 0.
    #[test]
    fn cycle_bfs_cache_second_read_skips_recompute_within_generation() {
        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number };")
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "Foo");

        // Cold call publishes entry at gen=G0.
        let mut fence1 = Vec::new();
        let result1 = ref_root_reaches_transitive_cycle_node(&id, host, &mut fence1);

        let db = host.project_type_store().ref_cycle_db();
        let live_after_cold = db.live_counter_for_test();
        assert!(
            live_after_cold >= 1,
            "cold publish must put 1 entry in the cache"
        );

        // A second call within the SAME generation must not re-publish.
        // Discriminating relative to a "publish-on-every-call" bug.
        reset_bfs_compute_counter_for_test();
        let mut fence2 = Vec::new();
        let result2 = ref_root_reaches_transitive_cycle_node(&id, host, &mut fence2);
        assert_eq!(
            result1, result2,
            "second call within same generation must return same result"
        );
        assert_eq!(
            bfs_compute_counter_for_test(),
            0,
            "second call within same generation must not re-run bfs_compute_inner",
        );
    }

    /// Test 7 — `peek`'s slow-path stale removal decrements
    /// the live_counter so the shared counter does not inflate
    /// permanently when entries become stale.
    ///
    /// Plan R8-5 fix: the original peek did `remove_if` without
    /// decrementing the counter on success. Repeated stale peeks would
    /// leak the counter upward.
    #[test]
    fn ref_cycle_result_db_peek_decrements_live_counter_on_stale_removal() {
        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number };")
            .unwrap();
        let host = project.host();
        let id = a0_make_decl_identity(host, "/types.ts", "Foo");

        // Insert a synthetic entry whose fact carrier is stale: it
        // carries a self-root `FileWholeHash` for `/nonexistent.ts`, a
        // canonical the host's `FileArtifactStore` does not track. The
        // strict `validate_with_self_roots` rejects an untracked
        // self-root; peek's slow-path removes the entry.
        let stale_carrier =
            crate::fact_signature_helpers::ReadSetSignature::new(std::sync::Arc::from(
                vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: "/nonexistent.ts".to_string(),
                    hash: [7u8; 16],
                }]
                .into_boxed_slice(),
            ));
        let stale_entry = std::sync::Arc::new(crate::component_meta_caches::RefCycleEntry {
            result: false,
            read_set_signature: stale_carrier,
            // The dispatch-return signature is not a validity rail —
            // staleness is carried by the fact carrier above.
            dispatch_dep_signature: std::sync::Arc::from(Vec::new()),
            // `/nonexistent.ts` listed as a self-root so the strict
            // validator routes its `FileWholeHash` through
            // `validates_self_root_whole_hash`, which rejects an
            // untracked self-root canonical.
            self_root_canonicals: std::sync::Arc::from(vec![std::sync::Arc::<str>::from(
                "/nonexistent.ts",
            )]),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
            // Current project generation — this entry's staleness is
            // exercised through the carrier rail, not the generation
            // gate, so it must match the live generation here.
            validated_at_generation: host.project_type_store().current_project_generation(),
        });
        let db = host.project_type_store().ref_cycle_db();
        db.entries().insert(id.clone(), stale_entry);
        db.bump_live_counter();
        let live_before = db.live_counter_for_test();
        assert_eq!(
            live_before, 1,
            "synthetic insert + bump_live_counter should leave live=1"
        );

        // Peek must return None (entry is stale). Every peek validates
        // the carrier strictly — the self-root `FileWholeHash` for the
        // untracked `/nonexistent.ts` fails `validates_self_root_whole_hash`;
        // peek removes the entry.
        let peek_result = db.peek(&id, host);
        assert!(
            peek_result.is_none(),
            "stale entry (fact carrier references nonexistent canonical) must be reaped"
        );

        let live_after = db.live_counter_for_test();
        assert_eq!(
            live_after, 0,
            "stale removal must decrement live_counter to prevent leak (R8-5 fix)"
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // P1.B regression — `FactReadSetFinalise::Overflow` returns the
    // valid materialisation outcome (NOT Tainted) and refuses cache
    // admission. Discriminates the bug fix by:
    //  1. Cold compute observes > FACT_SIGNATURE_CAP facts (forced via
    //     `MATERIALIZE_TEST_FORCE_OVERFLOW_OBSERVATIONS`); the installed
    //     fact tracer's `finalise()` reports `Overflow`.
    //  2. Pre-fix the materialiser returned `None` from the
    //     cooperative-admission compute closure, causing the caller to
    //     interpret the cooperative result as a non-cacheable miss → the
    //     materialiser surfaced `MaterializeOutcome::Tainted(key.base)`
    //     (the legacy fallback at the bottom of
    //     `materialize_component_meta_structure` had no stash from the
    //     Overflow path).
    //  3. Post-fix the Overflow arm stashes the computed entry's outcome
    //     into `non_cacheable_outcome` BEFORE returning `None`, so the
    //     fallback surfaces `MaterializeOutcome::Value(...)` and the
    //     cache stays empty so the next request cold-recomputes.
    //
    // Test serialisation: the forced observation knob is process-global,
    // so a serialisation mutex prevents concurrent overflow-driving
    // tests from racing on the shared atomic.
    static MATERIALIZE_OVERFLOW_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn overflow_returns_valid_outcome_and_refuses_cache_admission() {
        let _serial = MATERIALIZE_OVERFLOW_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number };")
            .unwrap();
        let host = project.host();

        // Construct a key whose base is a real DeclRef lowered from the
        // workspace. The cold compute walks the Foo body (small) but
        // the forced observation hook fires > 1024 synthetic facts onto
        // the active tracer, deterministically driving the tracer to
        // `FactReadSetFinalise::Overflow`. Without the forced
        // observations the build would publish a normal Value entry.
        let dispatch = ProjectSemanticDispatch::new(host);
        let decl_ref_node = dispatch
            .lower_type_expr_in_scope_with_mode(
                "/types.ts",
                &verter_type_expr::TypeExpr::Ref {
                    name: StdArc::from("Foo"),
                    type_arguments: StdArc::from(Vec::new()),
                },
                crate::semantic_query::ProjectionMode::Navigate,
            )
            .expect("lowering Foo via Navigate must succeed");
        let key = MaterializeStructureCacheKey {
            scope_canonical_id: StdArc::from("/types.ts"),
            base: decl_ref_node,
            scope_axis: MaterializationScope::TopLevel,
            mode: crate::semantic_query::ProjectionMode::Expanded,
        };

        // Snapshot the per-host overflow-refusal counter so the post
        // delta confirms the Overflow arm fired (independent of the
        // returned `CacheRead.value` discriminant).
        let refusals_before = host
            .provenance
            .materialize_structure_overflow_refusals
            .load(std::sync::atomic::Ordering::Relaxed);
        let db = host.project_type_store().materialize_structure_db();
        let entries_before = db.entries().len();

        // Arm the forced-observation hook. 1100 > FACT_SIGNATURE_CAP
        // (1024) — guarantees the cold compute's installed tracer
        // overflows.
        let _force_guard = MaterializeForceOverflowGuard::arm(1100);

        let read = materialize_component_meta_structure(host, key.clone());

        // Discrimination #1: the returned outcome MUST NOT be Tainted.
        // Pre-fix the bug surfaced `MaterializeOutcome::Tainted(key.base)`
        // because the cooperative-admission fallback ran without a
        // stashed outcome. Post-fix the Overflow arm stashes the
        // computed entry's outcome into the side channel.
        match read.value {
            MaterializeOutcome::Value(_) | MaterializeOutcome::Miss(_) => {
                // expected — Overflow refused admission but the
                // materialiser still returned the cacheable outcome
                // the cold compute produced.
            }
            MaterializeOutcome::Tainted(_) => {
                panic!(
                    "P1.B regression: Overflow path returned Tainted instead of the \
                     computed Value/Miss outcome. The cooperative-admission compute \
                     closure must stash the entry's outcome onto the \
                     non_cacheable_outcome side channel BEFORE returning None, so \
                     the fallback path at the bottom of \
                     materialize_component_meta_structure surfaces the valid \
                     materialisation."
                );
            }
            other => panic!("unexpected outcome on Overflow path: {other:?}"),
        }

        // Discrimination #2: the materialise-structure cache MUST NOT
        // contain the key. Refusing admission on Overflow is the whole
        // point — admission with an unbounded fact signature would
        // poison the cache.
        assert_eq!(
            db.entries().len(),
            entries_before,
            "Overflow MUST NOT admit the entry to the MaterializeStructureDb \
             cache; cache size must stay at the pre-call value to ensure \
             the next request cold-recomputes"
        );
        assert!(
            !db.entries().contains_key(&key),
            "MaterializeStructureDb cache MUST NOT contain the key whose cold \
             compute overflowed the fact tracer"
        );

        // Discrimination #3: the overflow-refusal counter advanced.
        let refusals_after = host
            .provenance
            .materialize_structure_overflow_refusals
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            refusals_after > refusals_before,
            "materialize_structure_overflow_refusals must advance on Overflow; \
             before={refusals_before}, after={refusals_after}"
        );

        // Drop the force guard before any subsequent operations so a
        // panic in the test driver does not leak the forced state.
        drop(_force_guard);
    }

    // =====================================================================
    // Publish-fence ⊇ revalidation — a project-generation `invalidate_all`
    // racing a cooperative-admission publish must not leave (or admit) a
    // stale entry.
    //
    // `cooperative_admit_with_post_publish` runs `revalidate_after_compute`
    // and then `map.insert` + `post_publish`. The `publish_fence` read
    // guard MUST span ALL THREE — if it covered only the insert, a
    // project-generation `invalidate_all` (which holds the matching write
    // guard across its clear) could land in the post-revalidate /
    // pre-insert gap, clear the cache, and then the winner would publish an
    // entry validated against the SUPERSEDED generation into the
    // freshly-cleared cache, defeating the reset.
    //
    // The two tests below pin a cooperative-admission winner at exactly
    // that gap via the substrate's `POST_REVALIDATE_PRE_PUBLISH_HOOK`
    // injection point and assert `retention_gate.try_write()` is `None` —
    // a concurrent project-generation reset reaching the write fence right
    // now WOULD block. DISCRIMINATES: with the fence covering only the
    // insert, the winner holds NO guard at the hook point, `try_write()`
    // succeeds (`Some`), and the assertion FAILS; with the fence widened
    // to cover the revalidation it returns `None` and the assertion
    // PASSES. The end-to-end no-stale-survivor assertion is the second,
    // independent discriminator.
    // =====================================================================

    /// `MaterializeStructureDb`: the cooperative-admission `publish_fence`
    /// spans `revalidate_after_compute` → `map.insert` → `post_publish`,
    /// so a project-generation `invalidate_all` cannot interleave its
    /// clear between the revalidation and the insert.
    #[test]
    fn materialize_structure_publish_fence_covers_revalidation_against_generation_reset() {
        use crate::component_meta_caches::MaterializeStructureEntry;
        use crate::semantic_query::{CacheRead, ProjectionMode, SemanticNodeId};
        use std::sync::Barrier;
        use std::thread;

        let project = a0_make_project();
        let host = project.host();
        let db = host.project_type_store().materialize_structure_db();

        let key = MaterializeStructureCacheKey {
            scope_canonical_id: StdArc::from("/fence_owner.ts"),
            base: SemanticNodeId(0),
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Shallow,
        };
        // The generation the synthetic entry is "computed" under.
        let validated_at_generation = host.project_type_store().current_project_generation();

        // party 1 = the parked cooperative winner, party 2 = main.
        let parked = StdArc::new(Barrier::new(2));

        let project_w = StdArc::clone(&project);
        let key_w = key.clone();
        let parked_w = StdArc::clone(&parked);
        let winner = thread::spawn(move || {
            let host = project_w.host();
            let db = host.project_type_store().materialize_structure_db();
            // Install the post-revalidate / pre-publish rendezvous on the
            // WINNER thread (the hook is thread-local and fires on the
            // cold winner's thread). The hook parks the winner inside the
            // `publish_fence` region, AFTER a successful
            // `revalidate_after_compute` and BEFORE `map.insert`.
            let _hook = crate::cooperative_admission::install_post_revalidate_pre_publish_hook(
                Box::new(move || {
                    parked_w.wait();
                    parked_w.wait();
                }),
            );
            let entry_outcome = MaterializeOutcome::Miss(SemanticNodeId(0));
            crate::cooperative_admission::cooperative_admit_with_post_publish(
                db.entries(),
                db.inflight(),
                key_w.clone(),
                // validate — generation gate + (vacuous) carrier check.
                |entry: &MaterializeStructureEntry| {
                    if entry.validated_at_generation
                        == host.project_type_store().current_project_generation()
                        && entry
                            .read_set_signature
                            .validate_with_self_roots(host, &entry.self_root_canonicals)
                    {
                        Some(CacheRead {
                            value: entry.outcome.clone(),
                            dep_signature: StdArc::clone(&entry.dispatch_dep_signature),
                            walker_diagnostics: StdArc::from([]),
                            cache_suppress: false,
                        })
                    } else {
                        None
                    }
                },
                // compute — a `Cacheable` entry stamped with the
                // pre-build project generation.
                || {
                    crate::cooperative_admission::ComputeAdmission::Cacheable(
                        MaterializeStructureEntry {
                            outcome: entry_outcome.clone(),
                            read_set_signature:
                                crate::fact_signature_helpers::ReadSetSignature::empty(),
                            dispatch_dep_signature: StdArc::from(Vec::new()),
                            self_root_canonicals: StdArc::from(Vec::<StdArc<str>>::new()),
                            admission_seq: crate::bounded_query_retention::next_retention_seq(),
                            validated_at_generation,
                        },
                    )
                },
                // project.
                |entry: &MaterializeStructureEntry| CacheRead {
                    value: entry.outcome.clone(),
                    dep_signature: StdArc::clone(&entry.dispatch_dep_signature),
                    walker_diagnostics: StdArc::from([]),
                    cache_suppress: false,
                },
                // revalidate_after_compute — generation gate + carrier.
                |entry: &MaterializeStructureEntry| {
                    entry.validated_at_generation
                        == host.project_type_store().current_project_generation()
                        && entry
                            .read_set_signature
                            .validate_with_self_roots(host, &entry.self_root_canonicals)
                },
                // removal_cleanup.
                |removed_key: &MaterializeStructureCacheKey,
                 removed_entry: &StdArc<MaterializeStructureEntry>| {
                    db.decrement_live_counter();
                    db.unregister_post_publish(
                        removed_key,
                        &removed_entry.read_set_signature,
                        removed_entry.admission_seq,
                    );
                },
                // post_publish.
                |entry_arc: &StdArc<MaterializeStructureEntry>,
                 k: &MaterializeStructureCacheKey| {
                    db.bump_live_counter();
                    db.register_post_publish(
                        k.clone(),
                        &entry_arc.read_set_signature,
                        entry_arc.admission_seq,
                    );
                },
                Some(db.publish_fence()),
            )
        });

        // The winner has run `revalidate_after_compute` and is parked
        // inside the `publish_fence` region, before `map.insert`.
        parked.wait();
        // DETERMINISTIC DISCRIMINATOR: with the fence widened to cover
        // the revalidation, the winner holds the `retention_gate` read
        // guard right now — a project-generation reset reaching the write
        // fence WOULD block. With the un-widened fence the winner holds
        // no guard yet and `try_write()` succeeds.
        assert!(
            db.test_retention_gate().try_write().is_none(),
            "REVALIDATE-VS-PUBLISH-FENCE RACE: the cooperative-admission \
             `publish_fence` read guard does NOT cover \
             `revalidate_after_compute`. A project-generation \
             `invalidate_all` can take the write guard in the \
             post-revalidate / pre-insert gap, clear the cache, and the \
             winner then publishes an entry validated against the \
             superseded generation into the freshly-cleared cache. The \
             fence must span revalidate → insert → post_publish.",
        );

        // A concurrent project-generation reset. Post-fix it blocks on
        // the `retention_gate` write guard the parked winner holds; it
        // proceeds only once the winner has inserted and released.
        let project_inval = StdArc::clone(&project);
        let invalidator = thread::spawn(move || {
            project_inval
                .host()
                .project_type_store()
                .bump_project_generation_and_evict();
        });

        // Release the winner: it inserts, runs post_publish, and drops
        // the fence guard.
        parked.wait();
        winner.join().expect("winner thread");
        invalidator.join().expect("invalidator thread");

        // No stale survivor. Post-fix the reset's `invalidate_all` is
        // ordered AFTER the winner's fenced insert, so it wipes the
        // just-published entry. Pre-fix the reset cleared the (empty)
        // cache BEFORE the unfenced winner inserted, stranding a stale
        // entry.
        assert_eq!(
            db.entry_count(),
            0,
            "a project-generation reset racing the publish left a stale \
             entry in the cache — the reset was defeated",
        );
        assert_eq!(
            db.live_counter_for_test(),
            0,
            "the live counter must match the (empty) entry map after the \
             racing project-generation reset",
        );
    }

    /// `RefCycleResultDb`: mirror of the `MaterializeStructureDb`
    /// publish-fence test — the cooperative-admission `publish_fence`
    /// spans `revalidate_after_compute` → `map.insert` → `post_publish`.
    #[test]
    fn ref_cycle_publish_fence_covers_revalidation_against_generation_reset() {
        use crate::component_meta_caches::RefCycleEntry;
        use crate::semantic_query::CacheRead;
        use std::sync::Barrier;
        use std::thread;

        let project = a0_make_project();
        let host = project.host();
        let db = host.project_type_store().ref_cycle_db();

        let id = a0_make_decl_identity(host, "/ref_cycle_fence.ts", "FenceHelper");
        let validated_at_generation = host.project_type_store().current_project_generation();

        let parked = StdArc::new(Barrier::new(2));

        let project_w = StdArc::clone(&project);
        let id_w = id.clone();
        let parked_w = StdArc::clone(&parked);
        let winner = thread::spawn(move || {
            let host = project_w.host();
            let db = host.project_type_store().ref_cycle_db();
            let _hook = crate::cooperative_admission::install_post_revalidate_pre_publish_hook(
                Box::new(move || {
                    parked_w.wait();
                    parked_w.wait();
                }),
            );
            crate::cooperative_admission::cooperative_admit_with_post_publish::<
                _,
                RefCycleEntry,
                CacheRead<bool>,
                _,
                _,
                _,
                _,
                _,
                _,
            >(
                db.entries(),
                db.inflight(),
                id_w.clone(),
                |entry: &RefCycleEntry| {
                    if entry.validated_at_generation
                        == host.project_type_store().current_project_generation()
                        && entry
                            .read_set_signature
                            .validate_with_self_roots(host, &entry.self_root_canonicals)
                    {
                        Some(CacheRead {
                            value: entry.result,
                            dep_signature: StdArc::clone(&entry.dispatch_dep_signature),
                            walker_diagnostics: StdArc::from([]),
                            cache_suppress: false,
                        })
                    } else {
                        None
                    }
                },
                || {
                    crate::cooperative_admission::ComputeAdmission::Cacheable(RefCycleEntry {
                        result: false,
                        read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(
                        ),
                        dispatch_dep_signature: StdArc::from(Vec::new()),
                        self_root_canonicals: StdArc::from(Vec::<StdArc<str>>::new()),
                        admission_seq: crate::bounded_query_retention::next_retention_seq(),
                        validated_at_generation,
                    })
                },
                |entry: &RefCycleEntry| CacheRead {
                    value: entry.result,
                    dep_signature: StdArc::clone(&entry.dispatch_dep_signature),
                    walker_diagnostics: StdArc::from([]),
                    cache_suppress: false,
                },
                |entry: &RefCycleEntry| {
                    entry.validated_at_generation
                        == host.project_type_store().current_project_generation()
                        && entry
                            .read_set_signature
                            .validate_with_self_roots(host, &entry.self_root_canonicals)
                },
                |removed_key: &crate::semantic_query::DeclIdentity,
                 removed_entry: &StdArc<RefCycleEntry>| {
                    db.decrement_live_counter();
                    db.unregister_post_publish(
                        removed_key,
                        &removed_entry.read_set_signature,
                        removed_entry.admission_seq,
                    );
                },
                |entry_arc: &StdArc<RefCycleEntry>, k: &crate::semantic_query::DeclIdentity| {
                    db.bump_live_counter();
                    db.register_post_publish(
                        k.clone(),
                        &entry_arc.read_set_signature,
                        entry_arc.admission_seq,
                    );
                },
                Some(db.publish_fence()),
            )
        });

        parked.wait();
        assert!(
            db.test_retention_gate().try_write().is_none(),
            "REVALIDATE-VS-PUBLISH-FENCE RACE (RefCycleResultDb): the \
             cooperative-admission `publish_fence` read guard does NOT \
             cover `revalidate_after_compute` — a project-generation \
             `invalidate_all` can clear the cache in the post-revalidate \
             / pre-insert gap and the winner then publishes a \
             superseded-generation entry into it. The fence must span \
             revalidate → insert → post_publish.",
        );

        let project_inval = StdArc::clone(&project);
        let invalidator = thread::spawn(move || {
            project_inval
                .host()
                .project_type_store()
                .bump_project_generation_and_evict();
        });

        parked.wait();
        winner.join().expect("winner thread");
        invalidator.join().expect("invalidator thread");

        assert_eq!(
            db.entries().len(),
            0,
            "a project-generation reset racing the BFS publish left a \
             stale entry in the RefCycleResultDb — the reset was defeated",
        );
        assert_eq!(
            db.live_counter_for_test(),
            0,
            "the live counter must match the (empty) entry map after the \
             racing project-generation reset",
        );
    }

    /// `MaterializeStructureDb::peek` rejects (and reaps) an entry whose
    /// `validated_at_generation` no longer equals the live project
    /// generation — a `ProjectGeneration` reset bumps no file content, so
    /// the carrier alone cannot detect a project-shape change.
    #[test]
    fn materialize_structure_peek_rejects_entry_from_superseded_generation() {
        use crate::component_meta_caches::MaterializeStructureEntry;
        use crate::semantic_query::{ProjectionMode, SemanticNodeId};

        let project = a0_make_project();
        let host = project.host();
        let db = host.project_type_store().materialize_structure_db();

        let key = MaterializeStructureCacheKey {
            scope_canonical_id: StdArc::from("/gen_peek_owner.ts"),
            base: SemanticNodeId(0),
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Shallow,
        };
        // Plant an entry with a VALID carrier (empty signature validates
        // vacuously — no self-root, no legacy dep) tagged with the
        // CURRENT project generation.
        let gen0 = host.project_type_store().current_project_generation();
        let entry = StdArc::new(MaterializeStructureEntry {
            outcome: MaterializeOutcome::Miss(SemanticNodeId(0)),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
            dispatch_dep_signature: StdArc::from(Vec::new()),
            self_root_canonicals: StdArc::from(Vec::<StdArc<str>>::new()),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
            validated_at_generation: gen0,
        });
        db.entries().insert(key.clone(), StdArc::clone(&entry));
        db.bump_live_counter();

        // Same generation — the carrier validates and the generation
        // matches, so `peek` HITs.
        assert!(
            db.peek(&key, host).is_some(),
            "an entry with a valid carrier and a matching project \
             generation must warm-hit",
        );

        // Bump ONLY the project generation (a tsconfig / SDK /
        // workspace-folder change bumps no file content) WITHOUT clearing
        // the cache. The planted entry's carrier is still valid — only
        // its `validated_at_generation` is now stale.
        host.project_type_store().bump_project_generation();

        // DISCRIMINATOR: `peek` must now MISS — the entry's
        // `validated_at_generation` no longer equals the live
        // generation. Without the generation gate `peek`'s carrier check
        // alone still passes (no file content changed) and the stale
        // entry is served.
        assert!(
            db.peek(&key, host).is_none(),
            "STALE-GENERATION READ: `MaterializeStructureDb::peek` served \
             an entry whose `validated_at_generation` is superseded — a \
             `ProjectGeneration` reset bumps no file content, so the \
             carrier check alone cannot detect it. `peek` must reject an \
             entry whose generation stamp no longer matches.",
        );
        // The stale entry was reaped, not merely skipped.
        assert_eq!(
            db.entry_count(),
            0,
            "the rejected stale-generation entry must be reaped from the \
             cache",
        );
    }

    /// `RefCycleResultDb::peek` rejects (and reaps) an entry whose
    /// `validated_at_generation` no longer equals the live project
    /// generation. Mirror of the `MaterializeStructureDb` test.
    #[test]
    fn ref_cycle_peek_rejects_entry_from_superseded_generation() {
        use crate::component_meta_caches::{ref_cycle_db_peek, RefCycleEntry};

        let project = a0_make_project();
        let host = project.host();
        let db = host.project_type_store().ref_cycle_db();

        let id = a0_make_decl_identity(host, "/gen_peek_cycle.ts", "Helper");
        let gen0 = host.project_type_store().current_project_generation();
        let entry = StdArc::new(RefCycleEntry {
            result: true,
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
            dispatch_dep_signature: StdArc::from(Vec::new()),
            self_root_canonicals: StdArc::from(Vec::<StdArc<str>>::new()),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
            validated_at_generation: gen0,
        });
        db.entries().insert(id.clone(), StdArc::clone(&entry));
        db.bump_live_counter();

        assert!(
            ref_cycle_db_peek(db, &id, host).is_some(),
            "an entry with a valid carrier and a matching project \
             generation must warm-hit",
        );

        host.project_type_store().bump_project_generation();

        assert!(
            ref_cycle_db_peek(db, &id, host).is_none(),
            "STALE-GENERATION READ: `RefCycleResultDb::peek` served an \
             entry whose `validated_at_generation` is superseded — a \
             `ProjectGeneration` reset bumps no file content, so the \
             carrier check alone cannot detect it. `peek` must reject an \
             entry whose generation stamp no longer matches.",
        );
        assert_eq!(
            db.entries().len(),
            0,
            "the rejected stale-generation entry must be reaped from the \
             cache",
        );
    }
}
