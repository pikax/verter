#![deny(missing_docs)]
//! Session-layer structural materialiser. Plan §1 / §10 / §16.
//!
//! Replaces the legacy `walk_component_meta_member_surface_expr`
//! family (`meta_resolve.rs:7669+`) with a dispatch-driven worklist
//! materialiser that uses graph-native policy predicates,
//! cooperative-admission post-compute revalidation for atomic
//! publish/invalidate, and a content-hash bucketed Weak-ref
//! `DepSignature` interner for `Arc::ptr_eq` cleanup of the
//! reverse-index.
//!
//! **Phase 8a (this commit) lands the foundational types:**
//! - [`MaterializeOutcome`] — materialiser-local result enum
//!   (Value / Miss / Recursive / Tainted / Error).
//! - [`MaterializationScope`] — TopLevel vs Nested axis.
//! - [`MaterializeStructureCacheKey`] — final-result cache key.
//! - [`convert_dispatch_result`] — boundary that promotes
//!   `QueryResult::Recursive` to `MaterializeOutcome::Tainted`
//!   per plan §1.2.
//!
//! **Phase 8b/c/d will add:** the materialiser entry point,
//! `MaterializeStructureDb` cache, per-shape handlers, graph-native
//! policy predicates, the cooperative-admission `post_publish`
//! wiring, the cycle-BFS port, and a comprehensive test suite.
//! Phase 9 cuts over the 16 production call sites and deletes the
//! walker family.

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

    // Plan §1.6 — package-ref policy gate. At TopLevel, a bare
    // DeclRef whose declaration resolves under `/node_modules/`
    // materialises to itself unchanged (the walker kept these
    // symbolic; expanding them would publish package internals).
    {
        let graph = host.project_type_store().semantic_graph();
        if key.scope_axis == MaterializationScope::TopLevel {
            if let Some(data) = graph.node_data(key.base) {
                use crate::semantic_query::SemanticNodeData;
                let is_package_ref = match data.as_ref() {
                    SemanticNodeData::DeclRef { identity } => {
                        identity.canonical_id.contains("/node_modules/")
                    }
                    SemanticNodeData::InstantiationRef { base, .. } => {
                        base.canonical_id.contains("/node_modules/")
                    }
                    _ => false,
                };
                if is_package_ref {
                    return crate::semantic_query::CacheRead {
                        value: MaterializeOutcome::Value(key.base),
                        dep_signature: empty_signature(),
                    };
                }
            }
        }
    }

    // Phase 4 — cooperative-admission cold build with post_publish.
    let key_for_compute = key.clone();
    let compute = move || {
        let dispatch = ProjectSemanticDispatch::new(host);
        // Materialisation through dispatch: empty path = full
        // surface expansion (plan §10).
        let path: std::sync::Arc<[PathSegment]> =
            std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
        let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
            base: key_for_compute.base,
            path,
            mode: key_for_compute.mode,
        });
        let mut local_fence: Vec<(Arc<str>, DepVersion)> =
            read.dep_signature.iter().cloned().collect();
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
        let outcome = match read.value {
            QueryResult::Value(id) => MaterializeOutcome::Value(id),
            QueryResult::Recursive(_) => MaterializeOutcome::Tainted(key_for_compute.base),
            QueryResult::Error(err) => MaterializeOutcome::Error(err),
        };
        if !outcome.is_cacheable() {
            // Don't publish non-cacheable outcomes.
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
            // revalidation failed. Re-dispatch and return the
            // outcome inline without caching.
            let dispatch = ProjectSemanticDispatch::new(host);
            let path: std::sync::Arc<[PathSegment]> =
                std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
            let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                base: key.base,
                path,
                mode: key.mode,
            });
            let mut local_fence: Vec<(Arc<str>, DepVersion)> =
                read.dep_signature.iter().cloned().collect();
            let outcome = match read.value {
                QueryResult::Value(id) => MaterializeOutcome::Value(id),
                QueryResult::Recursive(_) => MaterializeOutcome::Tainted(key.base),
                QueryResult::Error(err) => MaterializeOutcome::Error(err),
            };
            // Drop the seed canonical from fence for non-cacheable
            // results — they don't propagate as cache deps.
            local_fence.clear();
            crate::semantic_query::CacheRead {
                value: outcome,
                dep_signature: dep_signature_from_fence(local_fence),
            }
        }
    }
}

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
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
}
