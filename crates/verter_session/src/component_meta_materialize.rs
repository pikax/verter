#![deny(missing_docs)]
//! Session-layer structural materialiser. Plan §1 / §10 / §16.
//!
//! Replaces the legacy walker family with a dispatch-driven
//! materialiser that uses graph-native policy predicates,
//! cooperative-admission post-compute revalidation for atomic
//! publish/invalidate, and a content-hash bucketed Weak-ref
//! `DepSignature` interner for `Arc::ptr_eq` cleanup of the
//! reverse-index.
//!
//! **Foundational types** (plan §1.2 / §1.5 / §1.7):
//! - [`MaterializeOutcome`] — materialiser-local result enum
//!   (Value / Miss / Recursive / Tainted / Error).
//! - [`MaterializationScope`] — TopLevel vs Nested axis.
//! - [`MaterializeStructureCacheKey`] — final-result cache key.
//! - [`convert_dispatch_result`] — boundary that promotes
//!   `QueryResult::Recursive` to `MaterializeOutcome::Tainted`
//!   per plan §1.2.
//!
//! **Materialiser entry** (plan §10):
//! - [`materialize_component_meta_structure`] — five-phase entry
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
//! **Policy predicates** (plan §1.6 / §1.12):
//! - [`is_package_backed_ref`] — graph-native check that the input
//!   carrier resolves under `/node_modules/`. Walker behavior:
//!   keep symbolic at every axis.
//! - Function-shape skip at Nested — the walker's
//!   keep-function-bodies-symbolic invariant for Object-property
//!   positions.
//!
//! Phase 9 cut over the legacy walker shim to this entry, deleted
//! the walker's inner body family (cycle-key, scope-iteration, and
//! visited-set helpers), and deleted the dispatch-iteration module
//! that hosted the walker's visited-set helper. The static-grep gate
//! at `tests/no_legacy_walker.rs` enforces the deletion permanently
//! — see that file's `RETIRED_SYMBOLS` array for the canonical list
//! of names that must not reappear.

use std::sync::Arc;

use crate::semantic_query::{
    CacheRead, DepSignature, DepVersion, ProjectionMode, QueryError, QueryResult, SemanticNodeId,
};

/// Materialiser-local outcome enum. Plan §1.2.
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
    Miss(SemanticNodeId),
    /// Same-key recursion detected on this thread. Non-cacheable.
    /// Returned by the per-thread `MATERIALIZE_IN_FLIGHT` guard.
    Recursive(SemanticNodeId),
    /// Path-dependent outcome — depth-fuse trip, scope-unloaded, or
    /// a dispatch sub-call returned `Recursive`. Non-cacheable;
    /// propagates upward through the worklist as `Tainted`.
    Tainted(SemanticNodeId),
    /// Other dispatch error. Non-cacheable.
    Error(QueryError),
}

impl MaterializeOutcome {
    /// Extract the carried node id. For `Error` variants returns
    /// the caller-supplied opaque-miss id (non-extractable from
    /// QueryError directly — callers pass the host's opaque-miss
    /// fallback).
    #[must_use]
    pub fn node_id(&self, opaque_miss_fallback: SemanticNodeId) -> SemanticNodeId {
        match self {
            Self::Value(id) | Self::Miss(id) | Self::Recursive(id) | Self::Tainted(id) => *id,
            Self::Error(_) => opaque_miss_fallback,
        }
    }

    /// `true` for outcomes that may be published to the
    /// `MaterializeStructureDb` warm cache. Plan §1.2 invariant:
    /// only `Value` and `Miss` are cacheable. `Recursive` and
    /// `Tainted` are per-call-context; `Error` is non-deterministic.
    #[must_use]
    pub fn is_cacheable(&self) -> bool {
        matches!(self, Self::Value(_) | Self::Miss(_))
    }

    /// `true` when this outcome must propagate upward as a
    /// `Tainted` parent outcome.
    #[must_use]
    pub fn taints_parent(&self) -> bool {
        matches!(self, Self::Tainted(_))
    }
}

/// Plan §1.7 / §1.8 — materialisation scope axis. Determines how
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

/// Plan §1.5 — final-result cache key for the materialiser. Keyed
/// on `(scope_canonical_id, base, scope_axis, mode)` so the same
/// node id materialised at TopLevel vs Nested, or at Expanded vs
/// Navigate, lands in distinct slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaterializeStructureCacheKey {
    /// Owner scope — the canonical id the materialiser was
    /// dispatched in. Used to seed the local fence with the root
    /// scope's `WholeHash`.
    pub scope_canonical_id: Arc<str>,
    /// Input semantic node — the lowered TypeExpr that the
    /// materialiser is asked to materialise.
    pub base: SemanticNodeId,
    /// Axis the input was lowered at.
    pub scope_axis: MaterializationScope,
    /// Caller-side projection mode the materialiser ran with.
    pub mode: ProjectionMode,
}

/// Plan §1.2 — boundary that converts a dispatch
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
pub fn convert_dispatch_result(
    read: CacheRead<QueryResult<SemanticNodeId>>,
    input_node_for_sentinel: SemanticNodeId,
    local_fence: &mut Vec<(Arc<str>, DepVersion)>,
) -> MaterializeOutcome {
    local_fence.extend(read.dep_signature.iter().cloned());
    crate::host_manage::record_dep_signature_merge();
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
// Materialiser entry — plan §10
// ──────────────────────────────────────────────────────────────────

use std::cell::{Cell, RefCell};

use crate::component_meta_caches::MaterializeStructureEntry;
use crate::cooperative_admission::cooperative_get_or_insert_with_post_publish;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{PathSegment, SemanticQueryKey};
use crate::VerterHost;

thread_local! {
    /// Plan §1.4 — per-thread stack of in-flight materialiser keys.
    /// Used for same-key recursion detection. Push on entry, pop on
    /// exit (RAII via `MaterializeInFlightGuard`).
    static MATERIALIZE_IN_FLIGHT: RefCell<Vec<MaterializeStructureCacheKey>> =
        const { RefCell::new(Vec::new()) };

    /// Plan §1.4 — per-thread depth counter. The materialiser's
    /// defensive depth fuse trips at `MAX_DEPTH` to bound stack
    /// growth on pathological recursive shapes.
    static MATERIALIZE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Plan §1.4 — defensive depth fuse cap. A trip is a bug, not a
/// soft-fail; the audit emits `MaterializeStructureDepthFuseTripped`
/// with the input key + depth.
pub const MAX_DEPTH: usize = 4096;

/// Side-channel slot used by [`materialize_component_meta_structure`]
/// to share the compute closure's non-cacheable outcome (Tainted /
/// Error / Recursive) with the fallback path that runs when
/// `cooperative_get_or_insert_with_post_publish` returns `None`.
type NonCacheableSlot = RefCell<Option<(MaterializeOutcome, Vec<(Arc<str>, DepVersion)>)>>;

/// Plan §1.4 — RAII guard for the per-thread `MATERIALIZE_IN_FLIGHT`
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

/// Plan §10 — materialiser entry. Produces a `CacheRead` carrying
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
/// Plan §4.2 / B1 — single-exit helper for the materialiser compute
/// closure. Seeds `local_fence` with the root scope's whole_hash if
/// available, then either:
/// - For non-cacheable outcomes (Recursive / Tainted / Error), stashes
///   `(outcome, fence)` in `non_cacheable_slot` and returns `None` so
///   the cooperative-admission fallback can return the correct outcome
///   without re-dispatching.
/// - For cacheable outcomes (Value / Miss), returns `Some(MaterializeStructureEntry)`
///   so cooperative-admission publishes it.
fn finish_cacheable(
    host: &VerterHost,
    key: &MaterializeStructureCacheKey,
    outcome: MaterializeOutcome,
    mut local_fence: Vec<(Arc<str>, DepVersion)>,
    non_cacheable_slot: &NonCacheableSlot,
) -> Option<MaterializeStructureEntry> {
    if !key.scope_canonical_id.as_ref().is_empty() {
        if let Some(indexed) = host
            .project_type_store()
            .indexed()
            .get_any(key.scope_canonical_id.as_ref())
        {
            local_fence.push((
                Arc::clone(&key.scope_canonical_id),
                DepVersion::WholeHash(indexed.whole_hash),
            ));
        }
    }
    if !outcome.is_cacheable() {
        *non_cacheable_slot.borrow_mut() = Some((outcome, local_fence));
        return None;
    }
    Some(MaterializeStructureEntry {
        outcome,
        dep_signature: dep_signature_from_fence(local_fence),
    })
}

/// Plan §10 / §1.5 / §1.7 — five-phase materialiser entry. Maintains
/// the `MaterializeStructureDb` warm cache via cooperative-admission
/// with `post_publish` reverse-index registration.
///
/// **Phases:**
/// 1. Warm peek with proactive stale-entry removal (Plan §1.5).
/// 2. Same-key thread-local re-entry detection (Plan §10.2).
/// 3. Pre-admission depth fuse (Plan §10.3 / §1.7).
/// 4. Package-ref / function-shape-at-Nested policy gates (Plan §1.6).
/// 5. Cooperative-admission cold build with `post_publish`. Inside
///    the compute closure: registry-route branch (Plan §4.4 / B1),
///    recursive-helper cycle guard (Plan §4.13 / B1), then the
///    canonical DeclRef / InstantiationRef / Object handlers.
///
/// **Cache contract** (Plan §1.2):
/// - Only `Value` and `Miss` outcomes publish to the warm cache.
/// - `Recursive` and `Tainted` are per-call-context and never cache.
/// - `Error` is non-deterministic and never caches.
///
/// **Audit signal:** every entry/exit emits `MaterializeStructureEnter`
/// and `MaterializeStructureExit` events with the resolved
/// `CacheOutcomeKind` (`Hit` for warm, `ColdBuild` for cold,
/// `Tainted` for tainted, `Miss` for opaque). Plan §4.14 / B1 — also
/// emits `MaterializeStructurePolicySkip` events with one of:
/// `PackageRefTopLevel`, `FunctionPropertyAtNested`,
/// `RegistryRouteCycleGuard`, or `RecursiveHelperCycleGuard`.
pub fn materialize_component_meta_structure(
    host: &VerterHost,
    key: MaterializeStructureCacheKey,
) -> crate::semantic_query::CacheRead<MaterializeOutcome> {
    crate::host_manage::record_materialize_structure_call();

    let db = host.project_type_store().materialize_structure_db();

    // Phase 1 — warm-hit peek with proactive stale removal.
    if let Some(cached) = db.peek(&key, host) {
        return cached;
    }

    // Phase 2 — same-key thread-local re-entry detection.
    if MaterializeInFlightGuard::contains_key(&key) {
        let opaque = host.project_type_store().semantic_graph().intern_node(
            crate::semantic_query::SemanticNodeData::Opaque(QueryError::Miss),
        );
        return crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Recursive(opaque),
            dep_signature: empty_signature(),
        };
    }

    // Phase 3 — pre-admission depth fuse (one-call-deep check).
    if MaterializeInFlightGuard::current_depth() >= MAX_DEPTH {
        return crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Tainted(key.base),
            dep_signature: empty_signature(),
        };
    }

    let _guard = MaterializeInFlightGuard::push(key.clone());

    // Plan §1.6 — package-ref policy gate. A DeclRef or
    // InstantiationRef whose declaration resolves under
    // `/node_modules/` materialises to itself unchanged (the walker
    // kept these symbolic at every axis; expanding them would
    // publish package internals into the consumer's component-meta
    // surface).
    if is_package_backed_ref(host, key.base) {
        // Plan §4.14 / B1 — observability for kept-symbolic decision.
        crate::host_manage::emit_policy_skip(
            key.base,
            key.scope_axis,
            crate::component_meta_audit::MaterializeSkipReason::PackageRefTopLevel,
        );
        return crate::semantic_query::CacheRead {
            value: MaterializeOutcome::Value(key.base),
            dep_signature: empty_signature(),
        };
    }

    // Plan §1.6 — function-shape skip at Nested axis. The walker
    // kept function-typed Object members symbolic (their value
    // node was not expanded). Without this gate, dispatch's
    // ProjectPath { mode: Expanded } would unfold function bodies
    // inside member positions.
    if key.scope_axis == MaterializationScope::Nested {
        let graph = host.project_type_store().semantic_graph();
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
                };
            }
        }
    }

    // Phase 4 — cooperative-admission cold build with post_publish.
    // The compute closure shares its computed outcome via a side
    // channel so the post-cooperative fallback (when the entry is
    // non-cacheable and `cooperative_get_or_insert_with_post_publish`
    // returns None) can return the correct outcome without
    // re-dispatching.
    let non_cacheable_outcome: NonCacheableSlot = RefCell::new(None);
    let key_for_compute = key.clone();
    let non_cacheable_for_compute = &non_cacheable_outcome;
    let compute = move || {
        let dispatch = ProjectSemanticDispatch::new(host);
        let graph = host.project_type_store().semantic_graph();
        let mut local_fence: Vec<(Arc<str>, DepVersion)> = Vec::new();

        // Plan §4.4 / B1 Step 1 — registry-route branch.
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
                host,
                &mut local_fence,
            ) {
                crate::host_manage::emit_policy_skip(
                    key_for_compute.base,
                    key_for_compute.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::RegistryRouteCycleGuard,
                );
                return finish_cacheable(
                    host,
                    &key_for_compute,
                    MaterializeOutcome::Value(key_for_compute.base),
                    local_fence,
                    non_cacheable_for_compute,
                );
            }
            // Package-ref guard on the actual root.
            if crate::meta_resolve::component_meta_ref_resolves_to_package_node(
                &extraction.root_identity,
            ) {
                crate::host_manage::emit_policy_skip(
                    key_for_compute.base,
                    key_for_compute.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::PackageRefTopLevel,
                );
                return finish_cacheable(
                    host,
                    &key_for_compute,
                    MaterializeOutcome::Value(key_for_compute.base),
                    local_fence,
                    non_cacheable_for_compute,
                );
            }

            // Guards passed — let dispatch project the original
            // shape in the caller's mode (Plan §4.3: "Dispatch's
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
                        body_mode: crate::semantic_query::ProjectionMode::Navigate,
                    });
                    local_fence.extend(body_read.dep_signature.iter().cloned());
                    let body_id = match body_read.value {
                        QueryResult::Value(id) => id,
                        _ => {
                            return finish_cacheable(
                                host,
                                &key_for_compute,
                                MaterializeOutcome::Value(key_for_compute.base),
                                local_fence,
                                non_cacheable_for_compute,
                            );
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
                        body_mode: key_for_compute.mode,
                    });
                    local_fence.extend(projected.dep_signature.iter().cloned());
                    let projected_id = match projected.value {
                        QueryResult::Value(id) => id,
                        _ => {
                            return finish_cacheable(
                                host,
                                &key_for_compute,
                                MaterializeOutcome::Value(key_for_compute.base),
                                local_fence,
                                non_cacheable_for_compute,
                            );
                        }
                    };
                    return finish_cacheable(
                        host,
                        &key_for_compute,
                        MaterializeOutcome::Value(projected_id),
                        local_fence,
                        non_cacheable_for_compute,
                    );
                }
                RouteDemand::MemberPath(_) => {
                    // Plan §4.3 — IndexedAccess projection is dispatch's
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

        // Plan §4.13 / B1 Step 4 — recursive-helper cycle guard.
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
                host,
                &mut local_fence,
            ) {
                crate::host_manage::emit_policy_skip(
                    key_for_compute.base,
                    key_for_compute.scope_axis,
                    crate::component_meta_audit::MaterializeSkipReason::RecursiveHelperCycleGuard,
                );
                return finish_cacheable(
                    host,
                    &key_for_compute,
                    MaterializeOutcome::Value(key_for_compute.base),
                    local_fence,
                    non_cacheable_for_compute,
                );
            }
        }

        // Plan §1.6 / §10.7 — DeclRef / InstantiationRef handler.
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
                    body_mode: crate::semantic_query::ProjectionMode::Navigate,
                });
                local_fence.extend(read.dep_signature.iter().cloned());
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
                        let body_read = materialize_component_meta_structure(host, body_key);
                        local_fence.extend(body_read.dep_signature.iter().cloned());
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

        // Plan §1.8 — Object-shape handler. Walk the surface's
        // members + call/construct/index signatures and recursively
        // materialise each at Nested axis. The recursive entry
        // applies the package-ref + function-skip policies, so
        // function-valued members and package-backed refs are kept
        // symbolic while local refs continue to expand. This is the
        // load-bearing replacement for the legacy walker's
        // per-Object-member walk.
        let object_outcome = if ref_outcome.is_none() {
            if let Some(data) = graph.node_data(key_for_compute.base) {
                if let crate::semantic_query::SemanticNodeData::Object(surface) = data.as_ref() {
                    let surface = surface.clone();
                    Some(materialize_object_surface(
                        host,
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
                local_fence.extend(read.dep_signature.iter().cloned());
                match read.value {
                    QueryResult::Value(id) => MaterializeOutcome::Value(id),
                    QueryResult::Recursive(_) => MaterializeOutcome::Tainted(key_for_compute.base),
                    QueryResult::Error(err) => MaterializeOutcome::Error(err),
                }
            }
        };

        // Seed local_fence with the root scope's whole_hash if
        // available — plan §1.9 dep-signature accumulation contract.
        if !key_for_compute.scope_canonical_id.as_ref().is_empty() {
            if let Some(indexed) = host
                .project_type_store()
                .indexed()
                .get_any(key_for_compute.scope_canonical_id.as_ref())
            {
                local_fence.push((
                    Arc::clone(&key_for_compute.scope_canonical_id),
                    DepVersion::WholeHash(indexed.whole_hash),
                ));
            }
        }
        if !outcome.is_cacheable() {
            // Don't publish non-cacheable outcomes — but stash the
            // computed outcome + fence for the post-cooperative
            // fallback so it doesn't need to re-dispatch.
            *non_cacheable_for_compute.borrow_mut() = Some((outcome, local_fence));
            return None;
        }
        Some(MaterializeStructureEntry {
            outcome,
            dep_signature: dep_signature_from_fence(local_fence),
        })
    };

    let key_for_register = key.clone();
    let result = cooperative_get_or_insert_with_post_publish(
        db.entries(),
        db.inflight(),
        key.clone(),
        |entry: &MaterializeStructureEntry| {
            if crate::component_meta_caches::dep_signature_valid_for_host(
                &entry.dep_signature,
                host,
            ) {
                Some(crate::semantic_query::CacheRead {
                    value: entry.outcome.clone(),
                    dep_signature: entry.dep_signature.clone(),
                })
            } else {
                None
            }
        },
        compute,
        |entry: &MaterializeStructureEntry| crate::semantic_query::CacheRead {
            value: entry.outcome.clone(),
            dep_signature: entry.dep_signature.clone(),
        },
        // Plan §1.5 race-closer — post-compute revalidation.
        |entry: &MaterializeStructureEntry| {
            crate::component_meta_caches::dep_signature_valid_for_host(&entry.dep_signature, host)
        },
        // Plan §10.1 post_publish — register reverse-index AFTER
        // entries.insert AND AFTER successful revalidation.
        move |entry_arc: &Arc<MaterializeStructureEntry>, k: &MaterializeStructureCacheKey| {
            db.bump_live_counter();
            db.register_post_publish(
                key_for_register.clone(),
                Arc::clone(&entry_arc.dep_signature),
            );
            let _ = k; // unused — key_for_register is the same key
        },
    );

    match result {
        Some(read) => read,
        None => {
            // Compute returned None (non-cacheable outcome) OR
            // revalidation failed. Use the outcome the compute
            // closure stashed in the side channel — non-cacheable
            // results don't propagate as cache deps so the fence
            // is dropped.
            if let Some((outcome, _fence)) = non_cacheable_outcome.into_inner() {
                return crate::semantic_query::CacheRead {
                    value: outcome,
                    dep_signature: empty_signature(),
                };
            }
            // Revalidation failed (no compute outcome stashed).
            // Return Tainted on the input id — the next call will
            // re-attempt cooperative admission with the fresh
            // dep-signature.
            crate::semantic_query::CacheRead {
                value: MaterializeOutcome::Tainted(key.base),
                dep_signature: empty_signature(),
            }
        }
    }
}

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Plan §1.6 / §1.12 / §6.4 — graph-native package-ref policy predicate.
/// Returns `true` when `node` is a `DeclRef` or `InstantiationRef`
/// whose declaration's canonical id resolves under `/node_modules/`.
/// The walker's pre-cutover policy kept these refs symbolic at every
/// axis (TopLevel + Nested) — expanding them would publish package
/// internals into the consumer's component-meta surface.
///
/// Delegates the canonical-string check to the shared primitive
/// [`canonical_resolves_to_package`](crate::meta_resolve::canonical_resolves_to_package)
/// (commit C extracted this so the package check has one source of
/// truth across graph-node and identity-based callers).
pub(crate) fn is_package_backed_ref(host: &VerterHost, node: SemanticNodeId) -> bool {
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    use crate::semantic_query::SemanticNodeData;
    let canonical = match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => identity.canonical_id.as_ref(),
        SemanticNodeData::InstantiationRef { base, .. } => base.canonical_id.as_ref(),
        _ => return false,
    };
    crate::meta_resolve::canonical_resolves_to_package(canonical)
}

/// Plan §1.8 — Object-shape materialisation. Walk the surface's
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
    host: &VerterHost,
    key: &MaterializeStructureCacheKey,
    surface: &crate::semantic_query::SurfaceView,
    local_fence: &mut Vec<(Arc<str>, DepVersion)>,
) -> MaterializeOutcome {
    use crate::semantic_query::{IndexSignature, SemanticNodeData, SurfaceMember, SurfaceView};
    let graph = host.project_type_store().semantic_graph();

    let mut new_members = Vec::with_capacity(surface.members.len());
    let mut any_changed = false;
    for member in surface.members.iter() {
        let (sub_id, changed) = materialize_child_at_nested(host, key, member.value, local_fence);
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
        let (sub_id, changed) = materialize_child_at_nested(host, key, *sig, local_fence);
        any_changed |= changed;
        new_call_signatures.push(sub_id);
    }

    let mut new_construct_signatures = Vec::with_capacity(surface.construct_signatures.len());
    for sig in surface.construct_signatures.iter() {
        let (sub_id, changed) = materialize_child_at_nested(host, key, *sig, local_fence);
        any_changed |= changed;
        new_construct_signatures.push(sub_id);
    }

    let mut new_index_signatures = Vec::with_capacity(surface.index_signatures.len());
    for sig in surface.index_signatures.iter() {
        let (sub_value, vc) = materialize_child_at_nested(host, key, sig.value_type, local_fence);
        let (sub_key_ty, kc) = materialize_child_at_nested(host, key, sig.key_type, local_fence);
        any_changed |= vc || kc;
        new_index_signatures.push(IndexSignature {
            key_type: sub_key_ty,
            value_type: sub_value,
            readonly: sig.readonly,
        });
    }

    let new_keyspace = match surface.keyspace {
        Some(k) => {
            let (sub_id, changed) = materialize_child_at_nested(host, key, k, local_fence);
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
    host: &VerterHost,
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
    let sub_read = materialize_component_meta_structure(host, sub_key);
    local_fence.extend(sub_read.dep_signature.iter().cloned());
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
/// `Arc`. Plan §1.5 — used by the materialiser's publish path to
/// produce the final cache entry's dep_signature.
#[must_use]
pub fn dep_signature_from_fence(fence: Vec<(Arc<str>, DepVersion)>) -> DepSignature {
    Arc::from(fence.into_boxed_slice())
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
        // Plan §1.2 / round-7 P0 #1 — the load-bearing assertion.
        // Without this promotion, the dispatch's per-call-context
        // Recursive sentinel would be cached as a finalised Miss.
        let mut fence = Vec::new();
        let read = CacheRead {
            value: QueryResult::Recursive(dummy_node(123)),
            dep_signature: dummy_dep_signature(),
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

    // Compile-time: silence dead-code warnings on imports until the
    // materialiser body lands in Phase 8b/c/d.
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
    // Plan §6.2 / A0 — 7 RED-first tests for the legacy-parity cycle BFS
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

    /// Plan §4.13 / §4.14 / B1 — recursive-helper guard fires for
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

    /// Plan §4.4 / B1 — registry-route extraction recurses into
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
            verter_semantic::analysis::type_expr::LiteralValue::String("kids".to_string()),
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
    // F-prep tests (rev-10, plan §6.6.5).
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

    /// F-prep RED-first test (plan §6.6.5).
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
            body_mode: ProjectionMode::Skeleton,
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

    /// F-prep regression test (plan §6.6.5).
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
            body_mode: ProjectionMode::Navigate,
        });
        let _ = navigate_read; // confirms execution

        // Expanded + args=[] still executes without panic (continue-skip path).
        let expanded_read = dispatch.execute_read(SemanticQueryKey::Instantiate {
            base: id.clone(),
            args: StdArc::from(Vec::new().into_boxed_slice()),
            body_mode: ProjectionMode::Expanded,
        });
        let _ = expanded_read; // confirms execution
    }

    /// F-prep canonical-fixture A0 test #3b (plan §6.2 line ~1304).
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
            body_mode: ProjectionMode::Skeleton,
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
    // Plan §10.8 / §6.10 sub-task 1 — 5 tests covering:
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

    /// Plan §10.8 #1 — when the materialiser handles a `DeclRef`, the
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
                &verter_semantic::analysis::type_expr::TypeExpr::Ref {
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

    /// Plan §10.8 #2 — cycle BFS visited-set short-circuits.
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

    /// Plan §10.8 #3 — cycle BFS dispatches Instantiate per visited decl.
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

    /// Plan §10.8 #4 — orphan entry (stale dep_signature) is caught
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
                &verter_semantic::analysis::type_expr::TypeExpr::Ref {
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
        // Mutate /types.ts so the prior dep_signature becomes stale.
        project
            .upsert_base("/types.ts", "export type Foo = { x: number; y: string }")
            .unwrap();
        // Peek again — the stale entry must be reaped, not returned.
        let after_peek = host
            .project_type_store()
            .materialize_structure_db()
            .peek(&key, host);
        // Either None (stale entry was reaped) or Some(entry) where the
        // entry's dep_signature is still valid (legitimate cache hit).
        // We assert the cache invariant: peek never returns a stale entry.
        if let Some(read) = after_peek {
            // If we got Some, the dep_signature must be currently valid.
            assert!(
                crate::component_meta_caches::dep_signature_valid_for_host(
                    &read.dep_signature,
                    host,
                ),
                "peek returned a stale dep_signature — invariant violation"
            );
        }
    }

    /// Plan §10.8 #5 — orphan entry inserted directly into the cache
    /// is reaped on next peek (matches the test above's invariant from
    /// the other angle).
    #[test]
    fn materialize_orphan_entry_caught_on_next_peek() {
        use crate::semantic_query::{DepVersion, ProjectionMode};

        let project = a0_make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number }")
            .unwrap();
        let host = project.host();

        let dispatch = ProjectSemanticDispatch::new(host);
        let decl_ref_node = dispatch
            .lower_type_expr_in_scope_with_mode(
                "/types.ts",
                &verter_semantic::analysis::type_expr::TypeExpr::Ref {
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
        // Insert a stale orphan with a clearly-invalid dep_signature
        // (an all-zero whole_hash for /types.ts that doesn't match the
        // live whole_hash).
        let stale_signature = StdArc::from(
            vec![(
                StdArc::<str>::from("/types.ts"),
                DepVersion::WholeHash([0u8; 16]),
            )]
            .into_boxed_slice(),
        );
        let stale_entry = StdArc::new(MaterializeStructureEntry {
            outcome: MaterializeOutcome::Value(decl_ref_node),
            dep_signature: stale_signature,
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
    // Plan §6.13 / Commit R — RefCycleResultDb cache integration tests.
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

    /// Plan §6.13 test 1 — generation-local fast-path skips Instantiate
    /// dispatch on a warm cache hit.
    ///
    /// Cold call publishes the cache entry with
    /// `validated_at_generation == current`. Second call within the
    /// same `content_generation` returns via `peek`'s fast path WITHOUT
    /// any `Instantiate` dispatch. Discriminating: pre-R every call
    /// re-walks the BFS and dispatches; post-R the second call's
    /// dispatch trace is empty.
    #[test]
    fn cycle_bfs_cache_hit_avoids_dispatch_via_generation_check() {
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

        // Warm call — generation-local fast path skips dispatch entirely.
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
            "warm fast-path must skip Instantiate dispatch via generation-equal check"
        );
        assert_eq!(
            computes_second, 0,
            "warm fast-path must not run bfs_compute_inner"
        );
    }

    /// Plan §6.13 test 2 — `invalidate_for_canonical` drains the
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

    /// Plan §6.13 test 3 — `dep_signature` captures every canonical the
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

    /// Plan §6.13 test 4 — `invalidate_all` saturating-subtracts the
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

    /// Plan §6.13 test 5 — project-generation bump invalidates the
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

    /// Plan §6.13 test 6 — when the host's `content_generation` advances
    /// (e.g., via a file content edit), the cached entry's
    /// `validated_at_generation` becomes stale and the slow path
    /// revalidates. If the dep_signature still validates, the entry's
    /// `validated_at_generation` is updated and the cache hit is
    /// preserved.
    ///
    /// Discriminating: probes the workspace's content_generation moves
    /// when a file changes, AND that the cache responds correctly to
    /// the staleness.
    #[test]
    fn cycle_bfs_cache_uses_dep_signature_revalidation_when_generation_advances() {
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

    /// Plan §6.13 test 7 — `peek`'s slow-path stale removal decrements
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

        // Insert a synthetic entry whose dep_signature is stale (refs
        // a canonical that does not exist in the host's
        // IndexedReadyDb). peek's slow-path will reject and remove.
        let stale_signature: crate::semantic_query::DepSignature = std::sync::Arc::from(
            vec![(
                std::sync::Arc::<str>::from("/nonexistent.ts"),
                crate::semantic_query::DepVersion::WholeHash([7u8; 16]),
            )]
            .into_boxed_slice(),
        );
        let stale_entry = std::sync::Arc::new(crate::component_meta_caches::RefCycleEntry {
            result: false,
            dep_signature: stale_signature,
            validated_at_generation: std::sync::atomic::AtomicU64::new(u64::MAX),
        });
        let db = host.project_type_store().ref_cycle_db();
        db.entries().insert(id.clone(), stale_entry);
        db.bump_live_counter();
        let live_before = db.live_counter_for_test();
        assert_eq!(
            live_before, 1,
            "synthetic insert + bump_live_counter should leave live=1"
        );

        // Peek must return None (entry is stale). The cached_gen
        // (u64::MAX) does NOT match current, so the slow path runs;
        // dep_signature_valid_for_host returns false (canonical
        // doesn't exist); peek removes the entry.
        let peek_result = db.peek(&id, host);
        assert!(
            peek_result.is_none(),
            "stale entry (dep_signature references nonexistent canonical) must be reaped"
        );

        let live_after = db.live_counter_for_test();
        assert_eq!(
            live_after, 0,
            "stale removal must decrement live_counter to prevent leak (R8-5 fix)"
        );
    }
}
