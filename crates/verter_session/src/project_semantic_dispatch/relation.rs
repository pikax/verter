//! Relation engine for semantic-node assignability.
//!
//! The authoritative relation engine on the semantic graph. Results memoise
//! through [`SemanticGraphStore::insert_relation`] /
//! [`SemanticGraphStore::get_relation`] keyed by the FULL relation identity
//! [`crate::semantic_query::RelateMemoKey`] (source / target / relation kind /
//! policy / source freshness / inference context / env+substitution+
//! projection-reduction context), with dep-signature fencing. The current
//! engine computes [`RelationKind::Assignable`]; the other identity axes carry
//! their fixed default (per-axis algorithms land with the relation-inference
//! reducer, not yet implemented).
//!
//! All three [`RelationResult`] variants cache-with-fence:
//! `Assignable`/`NotAssignable`/`Unknown`. `Unknown` covers genuinely
//! undecidable judgements — deferred shells (`KeyOf`, `IndexedAccess`,
//! `Mapped`, `Conditional`, `TypeOf`, `TemplateLiteral`), cyclic
//! re-entry via [`RelationGuard`], and opaque carriers. The engine operates on
//! [`SemanticNodeData`] exclusively and never reaches into the arena. It is
//! path-independent: two callers reaching the same full relation identity see
//! the same memoised result.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::relation_predicates::*;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, DepSignature, InferBinding, LiteralValue, PrimitiveKind, QueryError, QueryResult,
    RelateMemoKey, RelationResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput, SurfaceView,
};
use crate::semantic_query_memo::SemanticGraphStore;

thread_local! {
    /// Per-thread relation in-flight (coinductive assumption) set, keyed by
    /// the FULL relation identity [`RelateMemoKey`] — NEVER the bare
    /// `(source, target)` pair. Cyclic re-entry on the same full identity
    /// returns `RelationResult::Unknown` per contract row without recursing.
    /// Keying on the full identity keeps two in-flight goals over the same
    /// nodes but a different relation kind / policy / freshness / inference
    /// context distinct, so the cycle guard does not over-merge them when the
    /// per-axis algorithms land with the relation-inference reducer (not yet
    /// implemented).
    ///
    /// Stack-safety is provided iteratively: structural fan-out
    /// (Alias unwrap, Union / Intersection distribution, Array /
    /// Tuple element descent) grows the heap-backed worklist rather
    /// than the Rust call stack, so pathological 1000-arm
    /// distribution does not risk `STATUS_STACK_OVERFLOW`.
    /// Termination is guaranteed by the graph-size-scaled work
    /// budget in `decide_relation` plus the in-flight set's cycle
    /// detection.
    static RELATION_IN_FLIGHT: RefCell<FxHashSet<RelateMemoKey>> =
        RefCell::new(FxHashSet::default());
}

/// Attempt to enter the relation guard for the full relation identity
/// `key`. Returns `true` on fresh entry (caller must call
/// [`exit_relation_guard`] before returning); returns `false` on cyclic
/// re-entry so the caller emits `Unknown` without infinite recursion.
fn enter_relation_guard(key: &RelateMemoKey) -> bool {
    RELATION_IN_FLIGHT.with(|cell| cell.borrow_mut().insert(key.clone()))
}

fn exit_relation_guard(key: &RelateMemoKey) {
    RELATION_IN_FLIGHT.with(|cell| {
        cell.borrow_mut().remove(key);
    });
}

/// Outcome of the identity-carrier unwrap performed before relation
/// dispatch.
///
/// `Concrete(id)` is the id the relation engine can compare
/// structurally (primitives, unions, objects, etc.); `Unresolvable`
/// maps to [`RelationResult::Unknown`] in the caller.
enum IdentityCarrierUnwrap {
    Concrete(SemanticNodeId),
    Unresolvable,
}

/// Canonical Record shapes the `Record<K, V>`-against-Object arm
/// handles.
///
/// `Record<K, V>` lowers to two different object surfaces depending on
/// whether K is a literal (union of literals) or a generic primitive.
enum RecordTargetShape {
    /// `Record<'ui', X>` / `Record<'ui' | 'prose', X>` — lowers to an
    /// `Object(SurfaceView)` with explicit members, one per literal
    /// key. The per-member value type is X; the Record semantics are
    /// structurally identical to the object surface.
    LiteralKey(SurfaceView),
    /// `Record<string, V>` / `Record<number, V>` — lowers to an
    /// `Object(SurfaceView)` with zero members and exactly one index
    /// signature whose key_type is the primitive key and value_type is
    /// V.
    GenericKey {
        key_type: SemanticNodeId,
        value_type: SemanticNodeId,
    },
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Relate `source` against `target`. Returns the tri-state
    /// relation judgement as a [`RelationResult`].
    ///
    /// All three outcomes memoise with dep-signature fencing via
    /// [`SemanticGraphStore::insert_relation`]. Warm hits short-circuit
    /// before the decision table runs.
    pub(crate) fn relate_nodes(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> (RelationResult, DepSignature) {
        let graph = self.graph();
        graph.record_relation_check();
        // The full relation identity: the current engine computes
        // ASSIGNABILITY under the live `R T L J` env, with default policy /
        // regular source freshness / no inference context. The relation kind /
        // policy / freshness / inference axes become live discriminators with
        // the relation-inference reducer (not yet implemented); today they take
        // their fixed default so the memo is
        // keyed on the full identity rather than the bare `(source, target)`
        // pair.
        let key = self.relate_memo_key(source, target);
        // Strict warm-hit fast path: a memoised judgement returns only
        // when its self-version-rooted carrier validates against the
        // live store view AND its `validated_at_generation` still
        // equals the live project generation — a content edit to the
        // source's or the target's originating file, OR a
        // project-shape change, misses the warm read and the
        // judgement recomputes below.
        if let Some((fence, cached)) = graph.get_relation(self.ctx, &key) {
            return (cached, fence);
        }
        // Snapshot the project generation BEFORE the cold compute
        // dispatches any work. The carrier validates only file-content
        // whole-hashes; a `ProjectGeneration` reset (tsconfig /
        // path-alias / SDK / workspace-folder change) bumps no file
        // content, so without this snapshot a `clear_relation_memo`
        // racing this `relate_nodes` could land a
        // stale-by-project-generation judgement whose carrier still
        // validates on file-content terms. The published entry stamps
        // this snapshot; `get_relation` rejects on warm read when the
        // live generation differs.
        let validated_at_generation = self.ctx.project_type_store().current_project_generation();
        let fence = self.project_generation_signature();
        // Cold compute under a nested fact tracer. The relation judgement
        // may instantiate identity carriers through the shared dispatch
        // (`unwrap_identity_carrier_for_relation` and the Object-vs-Record
        // arm both `execute(Instantiate …)`), so the result can depend on
        // imported / body facts beyond the `source` / `target` self-roots.
        // The tracer fans out into every active scope, so an enclosing cold
        // build still observes these facts; the inner read-set additionally
        // fences THIS relation memo entry on the transitive dependencies it
        // actually read. The cycle guard is keyed on the full relation
        // identity `key`, never the bare node pair.
        let host = self.ctx.host_for_fact_tracer_install();
        let (result, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
            // Test-only fact-injection hook. When the host's per-host
            // `relation_force_overflow_observations` knob is non-zero, emit
            // that many synthetic `FileWholeHash` observations onto the active
            // tracer so finalise reports `Overflow` once the per-signature cap
            // is exceeded — exercising the overflow non-admission path below
            // without a pathological multi-file fixture.
            let force_n = host
                .relation_force_overflow_observations
                .load(std::sync::atomic::Ordering::Relaxed);
            if force_n > 0 {
                for n in 0..force_n {
                    crate::resolver_core::resolver_context::observe_fan_out(
                        crate::resolver_core::FactVersionRef::FileWholeHash {
                            canonical_id: format!("__relation_force_overflow_{n}.ts"),
                            hash: [(n & 0xff) as u8; 16],
                        },
                    );
                }
            }
            if enter_relation_guard(&key) {
                let mut bindings: Vec<InferBinding> = Vec::new();
                let r = self.decide_relation_with_dispatch(source, target, &mut bindings);
                exit_relation_guard(&key);
                r
            } else {
                RelationResult::Unknown
            }
        });
        // Overflowed read-set: the judgement is returned to the caller but
        // refused memo admission — the dependency fence cannot be
        // represented — matching the cooperative cold-build contract.
        let traced_facts: &[crate::resolver_core::FactVersionRef] = match &finalise {
            crate::resolver_core::FactReadSetFinalise::Ok(facts) => facts,
            crate::resolver_core::FactReadSetFinalise::Overflow => return (result, fence),
        };
        // Self-version rooting: the relation judgement depends on the
        // `source` and `target` node surfaces plus the transitive facts
        // traced above — root the memo entry on the file content version
        // each file-derived input was lowered from and merge the traced
        // cross-file facts. A `None` carrier (a torn / conflicting self-root
        // observation, or a traced `FileWholeHash` that disagrees with the
        // observed self-root) makes the judgement non-cacheable: it is
        // returned to the caller but not admitted to the relation memo.
        let observed_self_roots = self.observed_self_roots_from_nodes([source, target]);
        let mut self_root_canonicals: Vec<std::sync::Arc<str>> =
            Vec::with_capacity(observed_self_roots.len());
        for (canonical, _) in observed_self_roots.iter() {
            if !self_root_canonicals.iter().any(|c| c == canonical) {
                self_root_canonicals.push(std::sync::Arc::clone(canonical));
            }
        }
        if let Some(carrier) = crate::semantic_query_memo::semantic_graph_read_set_signature(
            &observed_self_roots,
            traced_facts,
        ) {
            graph.insert_relation(
                key,
                carrier,
                std::sync::Arc::from(self_root_canonicals),
                result.clone(),
                validated_at_generation,
            );
        }
        (result, fence)
    }

    /// The full relation-memo key for `(source, target)` under the current
    /// engine's identity: assignability, default policy, regular source
    /// freshness, no inference context, the WORKSPACE-GLOBAL `R T L J` env
    /// (host-view sourced — the established one-engine convention; per-judgement
    /// env threading is a relation-inference-reducer concern (not yet
    /// implemented), not done here), the EMPTY
    /// canonical substitution, and the structural-transit reduction context. The
    /// SINGLE constructor `relate_nodes` uses to read/write the memo.
    pub(crate) fn relate_memo_key(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> crate::semantic_query::RelateMemoKey {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes();
        let context = crate::semantic_query::RelationContext {
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity().0,
            // Empty substitution + structural-transit reduction — see the doc.
            substitution: crate::semantic_query::SubstitutionCanonicalHash::empty(),
            projection_reduction:
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
        };
        crate::semantic_query::RelateMemoKey::assignable(source, target, context)
    }

    /// Dispatch-aware relation entry. Runs the Object-vs-Record arm
    /// BEFORE the core `decide_relation` authority.
    /// When the arm does not fire (source is not a workspace-scoped
    /// `DeclAnchor`, target is not a canonical Record shape, or the
    /// source unwrap produces a non-Object body), control falls through
    /// to `decide_relation` which handles the remaining dispatch. This
    /// gives the new arm visibility into the dispatcher (needed to
    /// `execute(Instantiate)` for DeclAnchor unwrap) without threading
    /// `&ProjectSemanticDispatch` through every free-function helper.
    ///
    /// **Identity-carrier unwrap.** Before calling the core
    /// `decide_relation` authority, any decl identity carrier on
    /// either side is instantiated into its concrete shape via
    /// [`Self::unwrap_identity_carrier_for_relation`]. The core
    /// authority assumes concrete shapes only; identity carriers are
    /// semantic anchors, not relation-comparable nodes. Unwrap failure
    /// (cycle / error / non-concrete result) surfaces as
    /// [`RelationResult::Unknown`], keeping the identity-carrier
    /// variant out of the relation arms themselves.
    fn decide_relation_with_dispatch(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        if let Some(r) = self.try_object_vs_record_relation(source, target, bindings) {
            return r;
        }
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        let target = match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        decide_relation(self.graph(), source, target, bindings)
    }

    /// Instantiate a decl identity carrier into its concrete shape
    /// for relation dispatch. Returns the id unchanged for nodes that
    /// are already concrete. Returns
    /// [`IdentityCarrierUnwrap::Unresolvable`] when the instantiation
    /// yields a cycle, an error, or still-non-concrete shape — the
    /// caller maps that to [`RelationResult::Unknown`].
    fn unwrap_identity_carrier_for_relation(&self, id: SemanticNodeId) -> IdentityCarrierUnwrap {
        let graph = self.graph();
        let Some(data) = graph.node_data(id) else {
            return IdentityCarrierUnwrap::Unresolvable;
        };
        let (identity, args): (DeclIdentity, Arc<[SemanticNodeId]>) = match &*data {
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => (
                DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                },
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            // Unwrap DeclRef/InstantiationRef carriers
            // (which is_deferred treats as deferred so build_conditional
            // doesn't prematurely close branches). The relation engine
            // here is allowed to materialise the concrete body to make a
            // definitive decision; if Instantiate yields Opaque, the
            // outer `is_deferred` treatment continues to apply.
            SemanticNodeData::DeclRef { identity } => (
                identity.clone(),
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            SemanticNodeData::InstantiationRef { base, args } => (base.clone(), Arc::clone(args)),
            _ => return IdentityCarrierUnwrap::Concrete(id),
        };
        drop(data);
        // The relation engine's identity-carrier unwrap is a
        // STRUCTURAL TRANSIT — the relation result is consumed for
        // assignability decisions, not published on a consumer-visible
        // surface. Dispatch the
        // instantiation under `StructuralTransit/Shallow` so nested
        // `keyof T` / `{ [K in S]: V }` operators carrier-stop and the
        // identity-carrier audit footprint does NOT reify per-member
        // anchors during binding.
        let transit = crate::semantic_query::ProjectionReductionContext::structural_transit();
        let unwrapped = match self.execute_type_node(SemanticQueryKey::Instantiate {
            base: self.type_slot_for(
                Arc::clone(&identity.canonical_id),
                Arc::clone(&identity.decl_name),
            ),
            context: self.instantiate_context_for(&identity.canonical_id, transit),
            args,
        }) {
            QueryResult::Value(SemanticQueryOutput {
                value: unwrapped, ..
            }) => self.evaluate_deferred_semantic_node_with_context(unwrapped, transit),
            _ => return IdentityCarrierUnwrap::Unresolvable,
        };
        let Some(unwrapped_data) = graph.node_data(unwrapped) else {
            return IdentityCarrierUnwrap::Unresolvable;
        };
        if matches!(
            &*unwrapped_data,
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
        ) {
            IdentityCarrierUnwrap::Unresolvable
        } else {
            drop(unwrapped_data);
            IdentityCarrierUnwrap::Concrete(unwrapped)
        }
    }

    /// Source-side `DeclPlaceholder` with Object body against
    /// target-side Object that looks like a `Record<K, V>` shape.
    /// Returns `Some(result)` when the arm applies; `None` to fall
    /// through to the core `decide_relation` authority.
    ///
    /// Fires only when:
    /// 1. `source` is an `Opaque(DeclPlaceholder)` whose
    ///    `canonical_id` is outside `node_modules/` (the walker's
    ///    package-boundary guard at `walk.rs` mirrors this).
    /// 2. `target` normalises to a Record-shaped `Object(SurfaceView)`.
    ///    Two canonical forms are recognised:
    ///    - **Literal-key Record** (`Record<'ui', X>` /
    ///      `Record<'ui' | 'prose', X>`): `view.members` is non-empty
    ///      and every member's value has the same shape (the
    ///      per-literal expansion). Call / construct signatures empty.
    ///    - **Generic-key Record** (`Record<string, V>` /
    ///      `Record<number, V>`): `view.members` is empty and
    ///      `view.index_signatures.len() == 1`. Call / construct
    ///      signatures empty.
    /// 3. `execute(Instantiate { base, args: empty })` resolves the
    ///    DeclPlaceholder to a body whose evaluation produces an
    ///    `Object(SurfaceView)`.
    ///
    /// The arm's recursive calls to `decide_relation` on per-member
    /// relations each re-enter the iterative worklist driver, which is
    /// bounded by its own termination budget (`10 × node_count`, 4096 floor),
    /// so per-member descent terminates.
    fn try_object_vs_record_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let graph = self.graph();
        let source_data = graph.node_data(source)?;
        let identity = match &*source_data {
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => {
                if self.ctx.workspace_is_package_backed(canonical_id) {
                    return None;
                }
                DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                }
            }
            _ => return None,
        };
        drop(source_data);

        let target_record = self.record_target_shape(target)?;

        // The Object-vs-Record arm is a STRUCTURAL TRANSIT — the
        // relation engine needs the
        // unwrapped Object surface, but the surface itself is consumed
        // for assignability, not published. Dispatch under
        // `StructuralTransit/Shallow` so the unwrap reads members /
        // index / call-construct signatures off a one-level surface
        // without re-reducing nested `keyof` / `Mapped` operators.
        let transit = crate::semantic_query::ProjectionReductionContext::structural_transit();
        let unwrapped = match self.execute_type_node(SemanticQueryKey::Instantiate {
            base: self.type_slot_for(
                Arc::clone(&identity.canonical_id),
                Arc::clone(&identity.decl_name),
            ),
            context: self.instantiate_context_for(&identity.canonical_id, transit),
            args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => {
                self.evaluate_deferred_semantic_node_with_context(id, transit)
            }
            _ => return Some(RelationResult::Unknown),
        };
        let source_view = match graph.node_data(unwrapped).as_deref() {
            Some(SemanticNodeData::Object(view)) => view.clone(),
            _ => return None,
        };

        Some(match target_record {
            RecordTargetShape::LiteralKey(target_view) => {
                relate_objects(graph, &source_view, &target_view, bindings)
            }
            RecordTargetShape::GenericKey {
                key_type,
                value_type,
            } => self.relate_object_as_record(&source_view, key_type, value_type, bindings),
        })
    }

    /// Returns `Some(RecordTargetShape)` when `target` normalises via
    /// `evaluate_deferred_semantic_node` to a Record-shaped
    /// `Object(SurfaceView)`. Two canonical forms are recognised
    ///: literal-key Record (members-only) and
    /// generic-key Record (single index signature, no members).
    fn record_target_shape(&self, target: SemanticNodeId) -> Option<RecordTargetShape> {
        let graph = self.graph();
        // The Object-vs-Record arm needs the target
        // normalised to a concrete `SemanticNodeData::Object`
        // (literal-key members surface or generic-key index
        // signature) to recognise a `Record<K, V>` shape — a `Mapped`
        // shell returned under `StructuralTransit` does not match.
        // Normalisation runs under the default `Published(Expanded)`
        // context so `Record<U, Record<K, any>>` reduces to its
        // Object surface for shape decision. Switching this call to
        // `StructuralTransit` would regress Record-target recognition
        // for `A extends Record<U, K>`-style conditionals — the
        // `neutral` overlay-augmented member in
        // `component_meta_host::tests::overlay_queries_reapply_owner_after_overlay_only_helper_upserts`
        // and the two sibling `ComponentConfig` materialisation
        // tests would stop seeing the augmented variant member.
        let normalised = self.evaluate_deferred_semantic_node(target);
        let data = graph.node_data(normalised)?;
        match &*data {
            SemanticNodeData::Object(view)
                if view.call_signatures.is_empty() && view.construct_signatures.is_empty() =>
            {
                if view.members.is_empty() && view.index_signatures.len() == 1 {
                    let ix = &view.index_signatures[0];
                    Some(RecordTargetShape::GenericKey {
                        key_type: ix.key_type,
                        value_type: ix.value_type,
                    })
                } else if !view.members.is_empty() && view.index_signatures.is_empty() {
                    Some(RecordTargetShape::LiteralKey(view.clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Relate an Object surface against a Record<K, V> target by
    /// checking that every required key (from
    /// the key type's literal enumeration) is present on the source
    /// and each matching member value is assignable to V.
    ///
    /// Key-type shapes handled:
    /// - `Literal(String(name))` — single required key.
    /// - `Union([Literal(String), ...])` — every literal key required.
    /// - `Primitive(String)` / `Primitive(Number)` — every source
    ///   member's value must be assignable to V.
    ///
    /// Any other key-type shape returns `Unknown` (defer the judgement
    /// rather than commit a wrong answer).
    fn relate_object_as_record(
        &self,
        source_view: &SurfaceView,
        key_type: SemanticNodeId,
        value_type: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let graph = self.graph();
        let key_data = match graph.node_data(key_type) {
            Some(d) => d,
            None => return RelationResult::Unknown,
        };
        let required_keys: Vec<Arc<str>> = match &*key_data {
            SemanticNodeData::Literal(LiteralValue::String(s)) => vec![Arc::from(s.as_str())],
            SemanticNodeData::Union(members) => {
                let members = Arc::clone(members);
                drop(key_data);
                let mut keys: Vec<Arc<str>> = Vec::with_capacity(members.len());
                for member in members.iter() {
                    match graph.node_data(*member).as_deref() {
                        Some(SemanticNodeData::Literal(LiteralValue::String(s))) => {
                            keys.push(Arc::from(s.as_str()));
                        }
                        _ => return RelationResult::Unknown,
                    }
                }
                keys
            }
            SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Number) => {
                // Generic-key Record<string, V> / Record<number, V>:
                // every source member's value must be assignable to V.
                drop(key_data);
                let mut acc = RelationResult::Assignable {
                    bindings: Arc::from(Vec::new().into_boxed_slice()),
                };
                for member in source_view.members.iter() {
                    let r = decide_relation(graph, member.value, value_type, bindings);
                    acc = result_and(acc, r);
                    if matches!(acc, RelationResult::NotAssignable) {
                        return RelationResult::NotAssignable;
                    }
                }
                return acc;
            }
            _ => return RelationResult::Unknown,
        };

        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for key in required_keys {
            let Some(member) = source_view
                .members
                .iter()
                .find(|m| m.name.as_ref() == key.as_ref())
            else {
                return RelationResult::NotAssignable;
            };
            let r = decide_relation(graph, member.value, value_type, bindings);
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        acc
    }
}

/// Iterative worklist item for [`decide_relation`].
///
/// The function is iterative, not recursive — pairs that need
/// structural descent push sub-pairs onto the worklist, and reducers
/// pop N prior results to combine them. See `expand_pair` for the
/// per-variant expansion rules.
#[derive(Debug, Clone)]
enum RelateWork {
    /// Evaluate `(source, target)`. Expands into either a direct result
    /// (pushed onto the result stack) or sub-work items + a reducer.
    Eval(SemanticNodeId, SemanticNodeId),
    /// Pop `n` prior results, AND them, push one combined result.
    ReduceAnd(u32),
    /// Pop `n` prior results, OR them, push one combined result.
    ReduceOr(u32),
}

/// Top-level iterative relation dispatch. Replaces the recursive
/// `decide_relation` / `decide_relation_inner` pair. Consumes a
/// worklist of pairs and reducers, publishing the final
/// [`RelationResult`] once the worklist drains.
///
/// Structural fan-out (Alias unwrap, Union / Intersection distribution,
/// Array / Tuple element descent) pushes sub-work onto the heap-backed
/// worklist rather than growing the Rust call stack. Compound-shape
/// helpers (`relate_objects`, `relate_function`, `relate_function_to_object`)
/// still delegate their remaining recursion to nested `decide_relation`
/// calls — each such call re-enters the iterative driver with a fresh
/// worklist, so stack growth is linear in compound-type nesting depth
/// rather than in union / intersection arm count.
///
/// `bindings` accumulates `infer` captures along the way; successful
/// `Assignable` results surface the bindings to the caller so
/// `build_conditional`'s true-branch substitution can pick them up.
///
/// **Termination budget.** The driver caps total work at
/// `10 × graph.node_count()` with a minimum floor of 4096 entries so tiny
/// graphs still handle pathological distributions. Exceeding the budget yields
/// [`RelationResult::Unknown`] rather than looping forever.
pub(super) fn decide_relation(
    graph: &SemanticGraphStore,
    source: SemanticNodeId,
    target: SemanticNodeId,
    bindings: &mut Vec<InferBinding>,
) -> RelationResult {
    if source == target {
        return assignable(bindings);
    }

    let budget_limit: u64 = (graph.node_count() as u64).saturating_mul(10).max(4096);
    let mut budget_used: u64 = 0;

    let mut work: Vec<RelateWork> = Vec::new();
    let mut results: Vec<RelationResult> = Vec::new();
    work.push(RelateWork::Eval(source, target));

    while let Some(item) = work.pop() {
        budget_used = budget_used.saturating_add(1);
        if budget_used > budget_limit {
            return RelationResult::Unknown;
        }

        match item {
            RelateWork::Eval(s, t) => {
                expand_pair(graph, s, t, bindings, &mut work, &mut results);
            }
            RelateWork::ReduceAnd(n) => {
                let combined = reduce_and_from_results(&mut results, n);
                results.push(combined);
            }
            RelateWork::ReduceOr(n) => {
                let combined = reduce_or_from_results(&mut results, n);
                results.push(combined);
            }
        }
    }

    results.pop().unwrap_or(RelationResult::Unknown)
}

fn reduce_and_from_results(results: &mut Vec<RelationResult>, n: u32) -> RelationResult {
    let mut combined = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    // bounded-loop: drains `n` per-pair results owned by this reducer — fan-out of the originating distribution; total work bounded by `decide_relation` budget (graph-size × 10).
    for _ in 0..n {
        let r = results
            .pop()
            .expect("RelateWork::ReduceAnd: result-stack underflow");
        combined = result_and(combined, r);
    }
    combined
}

fn reduce_or_from_results(results: &mut Vec<RelationResult>, n: u32) -> RelationResult {
    let mut combined = RelationResult::NotAssignable;
    // bounded-loop: drains `n` per-pair results owned by this reducer — fan-out of the originating distribution; total work bounded by `decide_relation` budget (graph-size × 10).
    for _ in 0..n {
        let r = results
            .pop()
            .expect("RelateWork::ReduceOr: result-stack underflow");
        combined = result_or(combined, r);
    }
    combined
}

/// Build a forward-ordered sequence of `RelateWork` items such that
/// after `push_forward_work`, the first item pops first (LIFO of the
/// worklist is preserved so that callers can read top-down execution
/// order from their `forward` vec).
fn push_forward_work(work: &mut Vec<RelateWork>, forward: Vec<RelateWork>) {
    for item in forward.into_iter().rev() {
        work.push(item);
    }
}

/// Expand a single relate pair into direct result(s) or sub-work items.
/// Pushes exactly one net result onto `results` by the time all
/// sub-work drains. Called from the driver loop per `RelateWork::Eval`.
fn expand_pair(
    graph: &SemanticGraphStore,
    source: SemanticNodeId,
    target: SemanticNodeId,
    bindings: &mut Vec<InferBinding>,
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
) {
    if source == target {
        results.push(assignable(bindings));
        return;
    }

    let source_data = match graph.node_data(source) {
        Some(d) => d,
        None => {
            results.push(RelationResult::Unknown);
            return;
        }
    };
    let target_data = match graph.node_data(target) {
        Some(d) => d,
        None => {
            results.push(RelationResult::Unknown);
            return;
        }
    };

    // ── Alias: unwrap transparently on either side ─────────────────────
    if let SemanticNodeData::Alias(inner) = &*source_data {
        let inner = *inner;
        drop(source_data);
        drop(target_data);
        work.push(RelateWork::Eval(inner, target));
        return;
    }
    if let SemanticNodeData::Alias(inner) = &*target_data {
        let inner = *inner;
        drop(source_data);
        drop(target_data);
        work.push(RelateWork::Eval(source, inner));
        return;
    }

    // ── MergedDecl: reduce to its peer-merged object surface on either side,
    //    then relate the merged surface (a merged interface relates exactly as
    //    its unified surface) ─────────────────────────────────────────────
    if let SemanticNodeData::MergedDecl { contributors } = &*source_data {
        let contributors = contributors.clone();
        drop(source_data);
        drop(target_data);
        let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
        work.push(RelateWork::Eval(merged, target));
        return;
    }
    if let SemanticNodeData::MergedDecl { contributors } = &*target_data {
        let contributors = contributors.clone();
        drop(source_data);
        drop(target_data);
        let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
        work.push(RelateWork::Eval(source, merged));
        return;
    }

    // ── Top / bottom ────────────────────────────────────────────────────
    match (&*source_data, &*target_data) {
        (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => {
            results.push(assignable(bindings));
            return;
        }
        (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => {
            results.push(assignable(bindings));
            return;
        }
        (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => {
            results.push(assignable(bindings));
            return;
        }
        (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => {
            results.push(assignable(bindings));
            return;
        }
        (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
            results.push(RelationResult::NotAssignable);
            return;
        }
        _ => {}
    }

    // ── Deferred shells on either side → Unknown ───────────────────────
    if is_deferred(&source_data) || is_deferred(&target_data) {
        results.push(RelationResult::Unknown);
        return;
    }

    // ── Error type (§22.3): an `Opaque(QueryError)` ERROR-TYPE carrier relates
    //    BIDIRECTIONALLY (like `any`), so a broken sub-result does not cascade
    //    spurious assignability failures. The error type is `ReturnOnly`-prone
    //    when input-degraded (a §18.4 property); RELATION-wise it is wildcard.
    //    Only the genuine error-type variants qualify (`QueryError::is_error_type`) —
    //    the control / recursion sentinels (`Miss`, `RecursiveRef`,
    //    `AliasCycle`, `BudgetExceeded`, `UnstableState`, `DeclPlaceholder`)
    //    keep their `Unknown` relation so recursion / resolution control flow
    //    is preserved. ──
    if matches!(
        &*source_data,
        SemanticNodeData::Opaque(err) if err.is_error_type()
    ) || matches!(
        &*target_data,
        SemanticNodeData::Opaque(err) if err.is_error_type()
    ) {
        results.push(assignable(bindings));
        return;
    }

    // ── Remaining opaque carriers (control / recursion sentinels) /
    //    VueMacroElements → Unknown (not the error type; relation cannot
    //    decide) ─────────────────────────────────────────────────────────────
    if matches!(
        &*source_data,
        SemanticNodeData::Opaque(_) | SemanticNodeData::VueMacroElements(_)
    ) || matches!(
        &*target_data,
        SemanticNodeData::Opaque(_) | SemanticNodeData::VueMacroElements(_)
    ) {
        results.push(RelationResult::Unknown);
        return;
    }

    // ── Type parameters: Unknown unless the pair is identical ─────────
    // Identity has already been checked at the top of expand_pair.
    if matches!(&*source_data, SemanticNodeData::TypeParam { .. })
        || matches!(&*target_data, SemanticNodeData::TypeParam { .. })
    {
        results.push(RelationResult::Unknown);
        return;
    }

    // ── Infer: defensive Unknown ───────────────────
    if matches!(&*source_data, SemanticNodeData::Infer { .. })
        || matches!(&*target_data, SemanticNodeData::Infer { .. })
    {
        results.push(RelationResult::Unknown);
        return;
    }

    // Identity carriers (DeclPlaceholder) are unwrapped in
    // `decide_relation_with_dispatch::unwrap_identity_carrier_for_relation`
    // before the core authority runs.

    // ── Union/Intersection distribution ────────────────────────────────
    if let SemanticNodeData::Union(members) = &*source_data {
        let members = Arc::clone(members);
        drop(source_data);
        drop(target_data);
        distribute_and(work, results, &members, |m| (*m, target));
        return;
    }
    if let SemanticNodeData::Union(members) = &*target_data {
        let members = Arc::clone(members);
        drop(source_data);
        drop(target_data);
        distribute_or(work, results, &members, |m| (source, *m));
        return;
    }
    if let SemanticNodeData::Intersection(members) = &*source_data {
        let members = Arc::clone(members);
        drop(source_data);
        drop(target_data);
        distribute_or(work, results, &members, |m| (*m, target));
        return;
    }
    if let SemanticNodeData::Intersection(members) = &*target_data {
        let members = Arc::clone(members);
        drop(source_data);
        drop(target_data);
        distribute_and(work, results, &members, |m| (source, *m));
        return;
    }

    // ── Primitives / literals ───────────────────────────────────────────
    if let (SemanticNodeData::Primitive(s), SemanticNodeData::Primitive(t)) =
        (&*source_data, &*target_data)
    {
        results.push(relate_primitives(*s, *t, bindings));
        return;
    }
    if let (SemanticNodeData::Literal(lit), SemanticNodeData::Primitive(prim)) =
        (&*source_data, &*target_data)
    {
        results.push(relate_literal_to_primitive(lit, *prim, bindings));
        return;
    }
    if let (SemanticNodeData::Literal(s), SemanticNodeData::Literal(t)) =
        (&*source_data, &*target_data)
    {
        results.push(if literals_equal(s, t) {
            assignable(bindings)
        } else {
            RelationResult::NotAssignable
        });
        return;
    }
    if matches!(&*source_data, SemanticNodeData::Primitive(_))
        && matches!(&*target_data, SemanticNodeData::Literal(_))
    {
        results.push(RelationResult::NotAssignable);
        return;
    }

    // ── Array / Tuple ──────────────────────────────────────────────────
    match (&*source_data, &*target_data) {
        (
            SemanticNodeData::Array {
                element: s_el,
                readonly: s_ro,
            },
            SemanticNodeData::Array {
                element: t_el,
                readonly: t_ro,
            },
        ) => {
            let (s_el, s_ro, t_el, t_ro) = (*s_el, *s_ro, *t_el, *t_ro);
            drop(source_data);
            drop(target_data);
            if !t_ro && s_ro {
                results.push(RelationResult::NotAssignable);
                return;
            }
            if t_ro || s_ro {
                // Covariant: forward only.
                work.push(RelateWork::Eval(s_el, t_el));
            } else {
                // Invariant: forward AND backward.
                let forward = vec![
                    RelateWork::Eval(s_el, t_el),
                    RelateWork::Eval(t_el, s_el),
                    RelateWork::ReduceAnd(2),
                ];
                push_forward_work(work, forward);
            }
            return;
        }
        (
            SemanticNodeData::Tuple {
                elements: s_els,
                readonly: s_ro,
            },
            SemanticNodeData::Tuple {
                elements: t_els,
                readonly: t_ro,
            },
        ) => {
            let s_els = Arc::clone(s_els);
            let t_els = Arc::clone(t_els);
            let s_ro = *s_ro;
            let t_ro = *t_ro;
            drop(source_data);
            drop(target_data);
            if !t_ro && s_ro {
                results.push(RelationResult::NotAssignable);
                return;
            }
            let required_target_len = t_els.iter().filter(|e| !e.optional && !e.rest).count();
            if s_els.len() < required_target_len {
                results.push(RelationResult::NotAssignable);
                return;
            }
            let pair_count = s_els.len().min(t_els.len());
            if pair_count == 0 {
                results.push(assignable(bindings));
                return;
            }
            let mut forward: Vec<RelateWork> = Vec::new();
            for (s_el, t_el) in s_els.iter().zip(t_els.iter()).take(pair_count) {
                if s_ro || t_ro {
                    forward.push(RelateWork::Eval(s_el.value, t_el.value));
                } else {
                    // Per-element bidirectional: Eval + Eval + ReduceAnd(2).
                    forward.push(RelateWork::Eval(s_el.value, t_el.value));
                    forward.push(RelateWork::Eval(t_el.value, s_el.value));
                    forward.push(RelateWork::ReduceAnd(2));
                }
            }
            if pair_count > 1 {
                forward.push(RelateWork::ReduceAnd(pair_count as u32));
            }
            push_forward_work(work, forward);
            return;
        }
        // Tuple ≤ Array (readonly): elementwise check.
        (
            SemanticNodeData::Tuple {
                elements: s_els,
                readonly: s_ro,
            },
            SemanticNodeData::Array {
                element: t_el,
                readonly: t_ro,
            },
        ) => {
            let s_els = Arc::clone(s_els);
            let s_ro = *s_ro;
            let t_el = *t_el;
            let t_ro = *t_ro;
            drop(source_data);
            drop(target_data);
            if !t_ro && s_ro {
                results.push(RelationResult::NotAssignable);
                return;
            }
            if s_els.is_empty() {
                results.push(assignable(bindings));
                return;
            }
            let mut forward: Vec<RelateWork> = Vec::with_capacity(s_els.len() + 1);
            for s in s_els.iter() {
                forward.push(RelateWork::Eval(s.value, t_el));
            }
            if s_els.len() > 1 {
                forward.push(RelateWork::ReduceAnd(s_els.len() as u32));
            }
            push_forward_work(work, forward);
            return;
        }
        _ => {}
    }

    // ── Function ───────────────────────────────────────────────────────
    if let (
        SemanticNodeData::Function {
            params: s_params,
            return_type: s_ret,
            ..
        },
        SemanticNodeData::Function {
            params: t_params,
            return_type: t_ret,
            ..
        },
    ) = (&*source_data, &*target_data)
    {
        let s_params = Arc::clone(s_params);
        let t_params = Arc::clone(t_params);
        let s_ret = *s_ret;
        let t_ret = *t_ret;
        drop(source_data);
        drop(target_data);
        // Function-shape helpers keep their internal `decide_relation`
        // calls — each re-enters the iterative driver with a fresh
        // worklist. Stack growth is linear in compound-type nesting
        // depth, not in function arity.
        results.push(relate_function(
            graph, &s_params, s_ret, &t_params, t_ret, bindings,
        ));
        return;
    }

    // ── Object structural (with heritage via SurfaceView) ──────────────
    if let (SemanticNodeData::Object(s_surf), SemanticNodeData::Object(t_surf)) =
        (&*source_data, &*target_data)
    {
        let s_surf = s_surf.clone();
        let t_surf = t_surf.clone();
        drop(source_data);
        drop(target_data);
        results.push(relate_objects(graph, &s_surf, &t_surf, bindings));
        return;
    }

    // ── Function source vs Object target with call signatures ─────────
    if let (
        SemanticNodeData::Function {
            params: s_params,
            return_type: s_ret,
            ..
        },
        SemanticNodeData::Object(t_surf),
    ) = (&*source_data, &*target_data)
    {
        let s_params = Arc::clone(s_params);
        let s_ret = *s_ret;
        let t_surf = t_surf.clone();
        drop(source_data);
        drop(target_data);
        results.push(relate_function_to_object(
            graph, &s_params, s_ret, &t_surf, bindings,
        ));
        return;
    }

    // Different concrete kinds → NotAssignable.
    results.push(RelationResult::NotAssignable);
}

/// Build and push the worklist fan-out for a source / target distribution
/// whose reducer is AND-all. Empty members push a direct `Assignable`
/// (vacuous truth — matches the accumulator's initial all-true state).
fn distribute_and<F>(
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
    members: &[SemanticNodeId],
    mut pairer: F,
) where
    F: FnMut(&SemanticNodeId) -> (SemanticNodeId, SemanticNodeId),
{
    let n = members.len();
    if n == 0 {
        results.push(RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        });
        return;
    }
    let mut forward: Vec<RelateWork> = Vec::with_capacity(n + 1);
    for m in members.iter() {
        let (s, t) = pairer(m);
        forward.push(RelateWork::Eval(s, t));
    }
    if n > 1 {
        forward.push(RelateWork::ReduceAnd(n as u32));
    }
    push_forward_work(work, forward);
}

/// Build and push the worklist fan-out for a source / target distribution
/// whose reducer is OR-any. Empty members push a direct `NotAssignable`
/// (matches the accumulator's initial all-false state).
fn distribute_or<F>(
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
    members: &[SemanticNodeId],
    mut pairer: F,
) where
    F: FnMut(&SemanticNodeId) -> (SemanticNodeId, SemanticNodeId),
{
    let n = members.len();
    if n == 0 {
        results.push(RelationResult::NotAssignable);
        return;
    }
    let mut forward: Vec<RelateWork> = Vec::with_capacity(n + 1);
    for m in members.iter() {
        let (s, t) = pairer(m);
        forward.push(RelateWork::Eval(s, t));
    }
    if n > 1 {
        forward.push(RelateWork::ReduceOr(n as u32));
    }
    push_forward_work(work, forward);
}
