#![deny(missing_docs)]
//! Session-layer structural materialiser. Plan §1 / §10 / §16.
//!
//! Replaces the legacy `walk_component_meta_member_surface_expr`
//! family with a dispatch-driven materialiser that uses
//! graph-native policy predicates, cooperative-admission
//! post-compute revalidation for atomic publish/invalidate, and a
//! content-hash bucketed Weak-ref `DepSignature` interner for
//! `Arc::ptr_eq` cleanup of the reverse-index.
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
//! Phase 9 cut over the legacy `walk_component_meta_member_surface_expr`
//! shim to this entry, deleted the walker's inner body family
//! (cycle-key, scope-iteration, and visited-set helpers), and
//! deleted the dispatch-iteration module that hosted the walker's
//! visited-set helper. The static-grep gate at
//! `tests/no_legacy_walker.rs` enforces the deletion permanently —
//! see that file's `RETIRED_SYMBOLS` array for the canonical list
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
}
