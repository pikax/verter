//! Graph-only symbolic-root predicates shared by the callable view, the
//! one-level surface projections and the framework slot-binding readers.
//!
//! A slot / callable parameter root is SYMBOLIC-ONLY when its binding identity
//! still depends on an unresolved generic, indexed, mapped or conditional
//! context; committing such a root to a concrete object surface would invent
//! bindings an undetermined generic context does not guarantee.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

/// Fact-tracer emission helper for the slot-binding traversal's dispatch
/// reads (the symbolic-root predicate here and the framework slot-binding
/// graph).
///
/// The slot-binding-graph traversal has no result cache of its own, so its
/// dispatch reads contribute evidence directly to the request-level tracer
/// that owns the reusable component-meta signature. The local counter lets
/// behavioral tests prove this traversal contributed evidence.
pub fn emit_slot_binding_graph_dispatch_facts<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    sig: &crate::semantic_query::DepSignature,
) {
    use std::sync::atomic::Ordering::Relaxed;
    if !sig.is_empty() {
        crate::request_observers::record_dep_signature_merge();
    }

    let bridged = crate::fact_signature_helpers::dep_signature_to_fact_signature(sig);
    crate::fact_signature_helpers::observe_fact_signature(&bridged);
    if let Some(prov) = dispatch.graph().provenance() {
        prov.slot_binding_graph_fact_tracer_emissions
            .fetch_add(1, Relaxed);
    }
}

/// Returns `true` when the slot-param's underlying shape is one that
/// the synthesis must NOT enumerate as a concrete object surface,
/// because the shape's binding identity depends on a generic /
/// indexed / mapped / conditional context that has not been resolved.
///
/// Walks through `Alias` and `InstantiationRef` (Skeleton-instantiated
/// body) hops to reach an effective root. Returns `true` for:
///
/// - `Conditional` (open or deferred — branch identity is undetermined
///   so neither branch's bindings are authoritative)
/// - `IndexedAccess` (e.g. `T["slot-name"]` — the indexed root has
///   not been resolved to a concrete shape)
/// - `Mapped`, `KeyOf`, `TemplateLiteral` (key-space transformations
///   with symbolic source)
/// - `TypeParam`, `Infer` (open generics)
///
/// Returns `false` for `Object`, `Function`, `Primitive`, `Literal`,
/// `Union`, `Intersection`, `Tuple`, `Array`, etc. — these are
/// directly enumerable by the empty-path Shallow walker.
///
/// The root is read one successor at a time — an alias's target, a concrete
/// conditional's reduction, an instantiated carrier's body — however long
/// the chain; a chain that returns to a node it passed is enumerable.
///
/// Shared so the framework DTO slot-binding extractors (which project a
/// slot-param object surface through the one-level shallow projection) apply
/// the SAME open-vs-concrete gate before materialising a slot-param object surface —
/// otherwise an open generic slot param (`SlotProps<M>` in a `generic="M"`
/// component) would reduce to a committed branch and the DTO path would invent a
/// phantom binding that the graph-native path correctly declined.
pub fn slot_param_root_is_symbolic_only<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    root: SemanticNodeId,
) -> bool {
    let mut node = root;
    let mut passed: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    while passed.insert(node) {
        match slot_param_root_step(dispatch, node) {
            SlotRootStep::Symbolic(symbolic) => return symbolic,
            SlotRootStep::Next(next) => node = next,
        }
    }
    false
}

/// One read of [`slot_param_root_is_symbolic_only`]: the node's own
/// answer, or the node it stands for.
enum SlotRootStep {
    Symbolic(bool),
    Next(SemanticNodeId),
}

fn slot_param_root_step<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> SlotRootStep {
    let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.graph(), node) else {
        return SlotRootStep::Symbolic(false);
    };
    SlotRootStep::Symbolic(match data.as_ref() {
        SemanticNodeData::Alias(inner) => return SlotRootStep::Next(*inner),
        // A Conditional is symbolic-only ONLY when it is genuinely OPEN — i.e.
        // its CHECK still contains a free `TypeParam` / `Infer` shell, so the
        // branch identity is undetermined. The distinction:
        //
        // - FREE generic (`[M] extends ['hover']` in a `generic="M extends Mode"`
        //   component): the check carries the free `TypeParam` `M`, so the
        //   conditional is OPEN. Neither branch's bindings are authoritative —
        //   classify symbolic. (A `Published(Shallow)` reduction WOULD commit to
        //   a branch here via `M`'s constraint, inventing a phantom binding —
        //   which is exactly the bug this guard prevents.)
        // - CONCRETE substitution (`{ id: string } extends { id: infer U }`): the
        //   check is fully concrete (no free `TypeParam`), so the conditional is
        //   DECIDABLE. Reduce it through an empty-path `Published(Shallow)`
        //   `ProjectPath` (the projection walker applies inference-binding +
        //   decidable-conditional reduction that a bare `SemanticQueryKey::
        //   Conditional` dispatch leaves deferred for an `infer`-bearing extends
        //   clause) and classify the reduced terminal.
        SemanticNodeData::Conditional { check, .. } => {
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let check = *check;
            drop(data);
            // Open check (free TypeParam / Infer) → genuinely symbolic.
            if node_contains_free_type_param(dispatch, check) {
                return SlotRootStep::Symbolic(true);
            }
            // Concrete check → reduce the conditional and classify the result.
            let empty_path: Arc<[crate::semantic_query::PathSegment]> =
                Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice());
            let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                base: node,
                path: empty_path,
                context: crate::semantic_query::ProjectionReductionContext::published(
                    ProjectionMode::Shallow,
                ),
            });
            crate::request_context::observe_component_meta_read_suppress(&read);
            emit_slot_binding_graph_dispatch_facts(dispatch, &read.dep_signature);
            match read.value {
                // Decidable: reduced to a concrete terminal — classify it (a
                // concrete branch root is enumerable, not symbolic).
                QueryResult::Value(reduced) if reduced != node => {
                    return SlotRootStep::Next(reduced)
                }
                // Stayed the deferred conditional shell (open / undecidable) —
                // genuinely symbolic-only.
                _ => true,
            }
        }
        SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::InferRef { .. } => true,
        SemanticNodeData::InstantiationRef { base, args } => {
            // Skeleton-instantiate the carrier under its OWN `(base, args)` so the
            // substitution actually binds — but unbound parameters become
            // `TypeParam` SHELLS, never their declared DEFAULT. This is the
            // distinction the generic slot-param gate turns on:
            //
            // - CONCRETE substitution (`SlotProps<{ id: string }>`): the args
            //   bind, so the body's Conditional / Mapped check is concrete and
            //   the Conditional arm below reduces it to a concrete root (bindings
            //   enumerated). The earlier EMPTY-args instantiation erased the
            //   substitution and left every such body an OPEN Conditional, so the
            //   rows were silently dropped.
            // - FREE generic (`SlotProps<M>` in a `generic="M extends Mode"`
            //   component): `M` stays a `TypeParam` shell (Skeleton does NOT
            //   apply the `= Mode` default), so the body's Conditional check is
            //   open and the recursion classifies it symbolic — declining to
            //   invent bindings from an undetermined generic context. (A
            //   `Published(Shallow)` projection of the carrier WOULD apply the
            //   default and wrongly commit to a branch — hence Skeleton, not
            //   ProjectPath, on the carrier.)
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
                dispatch.type_slot_for(
                    Arc::clone(&base.canonical_id),
                    base.owner,
                    Arc::clone(&base.decl_name),
                ),
                Arc::clone(args),
                dispatch.instantiate_context_for(
                    &base.canonical_id,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Skeleton,
                    ),
                ),
            ));
            let read = dispatch.execute_read(key);
            // Dual-emit: legacy accumulator + fact-tracer fan-out.
            crate::request_context::observe_component_meta_read_suppress(&read);
            emit_slot_binding_graph_dispatch_facts(dispatch, &read.dep_signature);
            match read.value {
                QueryResult::Value(body_id) if body_id != node => {
                    return SlotRootStep::Next(body_id)
                }
                // Did not instantiate past the carrier shell (or errored) —
                // an unresolvable carrier has no concrete enumerable root.
                _ => true,
            }
        }
        _ => false,
    })
}

/// Returns `true` when `node`'s structure still contains a FREE `TypeParam` or
/// `Infer` shell — i.e. the type is NOT fully concrete. Used by the slot-param
/// gate to decide whether a Conditional's check is decidable: a check carrying
/// a free type parameter (`[M] extends ['hover']` in a generic component) is
/// OPEN and must stay symbolic, while a fully-concrete check
/// (`{ id: string } extends { id: infer U }`) is decidable and may reduce.
///
/// Walks the compound shapes a substituted check can take (Tuple / Array /
/// Union / Intersection / Object members / KeyOf / IndexedAccess), resolving
/// one-level `Alias` hops, and descends the `type_args` of the structural
/// carriers (`BareRef` / `TypeOf` / `ImportType`) — a free `TypeParam` inside a
/// carrier's applied arguments keeps the check open. `Infer` shells inside the
/// conditional's EXTENDS clause are intentionally NOT inspected here (this
/// predicate is applied to the CHECK only) — an `infer U` in `extends` is the
/// binding mechanism, not an open check parameter. The lazy declaration carriers
/// (`DeclRef` / `InstantiationRef`) are treated as NOT-free (they are concrete
/// declaration references, resolved elsewhere). Every node is read once,
/// whatever the nesting.
pub fn node_contains_free_type_param<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> bool {
    use crate::graph_walk::Reach;
    crate::graph_walk::reaches(node, |node| {
        let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.graph(), node)
        else {
            return Reach::Parts(Vec::new());
        };
        Reach::Parts(match data.as_ref() {
            SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. } => return Reach::Hit,
            SemanticNodeData::Alias(inner) => vec![*inner],
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                composite
                    .composite_members()
                    .expect("composite arm")
                    .iter()
                    .copied()
                    .collect()
            }
            SemanticNodeData::Array { element, .. } => vec![*element],
            SemanticNodeData::Tuple { elements, .. } => {
                elements.iter().map(|element| element.value).collect()
            }
            SemanticNodeData::Object(view) => view
                .positive_members()
                .iter()
                .map(|member| member.value)
                .collect(),
            SemanticNodeData::KeyOf { base } => vec![*base],
            SemanticNodeData::IndexedAccess { object, .. } => vec![*object],
            // A `BareRef` / `TypeOf` / `ImportType` carrier applies its
            // arguments at the reference site; a free `TypeParam` inside
            // those args makes the node contain a free param (the check
            // stays open). Descend the args via the shared accessor
            // (args-only; the carrier head is not resolved).
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::ImportType(_) => data.carrier_type_args().to_vec(),
            // Primitives, literals, functions, opaque, etc. carry no free
            // open parameter on the check path.
            _ => Vec::new(),
        })
    })
}
