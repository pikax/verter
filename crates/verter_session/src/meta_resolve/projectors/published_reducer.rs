//! Reducible-operator structural predicate.
//!
//! This module classifies settled semantic nodes before the bounded fixed-point
//! reducer runs. The published-surface driver lives in the terminal
//! `output_sink`; these predicates inspect only the semantic graph and never
//! cross the output-materialization boundary.

use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;

use crate::resolver_core::ResolverContext;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::semantic_query_memo::SemanticGraphStore;

/// The shallow reduction-gate facts of a graph `node`, read in NODE DOMAIN —
/// the shape decisions used by the per-member publication path:
///
/// - `generic_instantiation_ref` — `raise(node)` is a `Ref` with NON-EMPTY type
///   arguments (`is_generic_instantiation`): a generic application that enters
///   the reducer and roots the cycle guard.
/// - `contains_reducible_operator` — the node or a descendant carries an
///   operator the fixed-point reducer owns.
///
/// Computed WITHOUT materialising a `TypeExpr`: the root classification reads the
/// in-hand carrier kind (peeling a single `Alias`), and the operator scan folds
/// the graph through [`node_contains_reducible_operator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeReductionGateFacts {
    pub(crate) generic_instantiation_ref: bool,
    pub(crate) contains_reducible_operator: bool,
}

/// Peel a single chain of graph-native `Alias` carriers so the root
/// classification reads the underlying shape (the reverse boundary unwraps an
/// `Alias` before raising, so the raised root is the aliased node's root).
fn peel_alias_root(
    graph: &SemanticGraphStore,
    mut node: SemanticNodeId,
    mut depth: u32,
) -> SemanticNodeId {
    while depth < 256 {
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Alias(inner)) => {
                node = *inner;
                depth += 1;
            }
            _ => break,
        }
    }
    node
}

/// Classify a graph `node`'s shallow reduction gates in node domain. The
/// node-domain replacement for the per-member path's
/// `matches!(raise(node), TypeExpr::Primitive | …)` /
/// `is_generic_instantiation` / `type_expr_contains_reducible_operator(raise(node))`
/// triad. Parity-checked field-for-field against the `TypeExpr` predicates on
/// `raise(node)`.
pub(crate) fn classify_node_reduction_gates(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> NodeReductionGateFacts {
    let graph = ctx.project_type_store().semantic_graph();
    // Peel a single `Alias` chain so the root classification reads the underlying
    // carrier kind (the reverse boundary unwraps an `Alias` before raising).
    let root = peel_alias_root(graph, node, 0);
    let generic_instantiation_ref = match graph.node_data(root).as_deref() {
        // `raise(InstantiationRef)` is `Ref` with NON-EMPTY type arguments.
        Some(SemanticNodeData::InstantiationRef { .. }) => true,
        // An unresolved `BareRef` raises to `Ref { name, type_args }`: a generic
        // instantiation iff it carries type arguments.
        Some(data) if data.bare_ref_head().is_some() => !data.carrier_type_args().is_empty(),
        _ => false,
    };
    NodeReductionGateFacts {
        generic_instantiation_ref,
        contains_reducible_operator: node_contains_reducible_operator(graph, node, 0),
    }
}

/// Whether `node` carries any operator-shape node (`IndexedAccess` / `KeyOf` /
/// `TypeOf` / `Conditional` / `Mapped` / `Infer` / `ImportType`) or an applied
/// builtin-utility reference, recursing through compound operands. Folds the
/// graph directly — NO `TypeExpr` materialised. `depth` fuses at 256.
pub(crate) fn node_contains_reducible_operator(
    graph: &SemanticGraphStore,
    node: SemanticNodeId,
    depth: u32,
) -> bool {
    if depth > 256 {
        return false;
    }
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    let recur = |n: SemanticNodeId| node_contains_reducible_operator(graph, n, depth + 1);
    match data.as_ref() {
        // Operator-shape carriers — the raised form is the deferred operator.
        SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::Infer { .. }
        // An import-type raises to `TypeExpr::ImportType` — the unconditional-true
        // arm of the `TypeExpr` predicate.
        | SemanticNodeData::ImportType(_) => true,
        // `raise(InstantiationRef)` is `Ref { name, type_arguments: [..] }`: a
        // builtin-utility name with arguments is reducible; otherwise recurse the
        // arguments (the `TypeExpr` fall-through `Ref` arm).
        SemanticNodeData::InstantiationRef { base, args } => {
            (!args.is_empty() && BuiltinUtility::from_name(base.decl_name.as_ref()).is_some())
                || args.iter().any(|&arg| recur(arg))
        }
        // A bare `Ref { name, type_args }`: same builtin-utility-or-recurse rule.
        SemanticNodeData::DeclRef { .. } => false,
        d if d.bare_ref_head().is_some() => {
            let args = d.carrier_type_args();
            (!args.is_empty()
                && d.bare_ref_head()
                    .is_some_and(|head| BuiltinUtility::from_name(head.0.as_ref()).is_some()))
                || args.iter().any(|&arg| recur(arg))
        }
        // Graph-native `Alias` — pass-through (no `TypeExpr` equivalent).
        SemanticNodeData::Alias(inner) => recur(*inner),
        SemanticNodeData::Array { element, .. } => recur(*element),
        SemanticNodeData::Tuple { elements, .. } => elements.iter().any(|el| recur(el.value)),
        SemanticNodeData::Union(members)
        | SemanticNodeData::Intersection(members)
        | SemanticNodeData::MergedDecl {
            contributors: members,
        } => members.iter().any(|&m| recur(m)),
        SemanticNodeData::Object(surface) => {
            surface.members.iter().any(|m| recur(m.value))
                || surface
                    .index_signatures
                    .iter()
                    .any(|sig| recur(sig.key_type) || recur(sig.value_type))
                || surface.call_signatures.iter().any(|&c| recur(c))
                || surface.construct_signatures.iter().any(|&c| recur(c))
        }
        SemanticNodeData::Function {
            params,
            return_type,
            ..
        } => params.iter().any(|p| recur(p.ty)) || recur(*return_type),
        SemanticNodeData::ConstructorType { signature } => recur(*signature),
        SemanticNodeData::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(|&e| recur(e))
        }
        _ => false,
    }
}
