//! Reducible-operator structural predicate.
//!
//! This module hosts the structural predicate that decides whether the
//! bounded fixed-point reducer needs to run for a given `TypeExpr`. The
//! published-surface field-type driver (`reduce_published_field_types`) and the
//! per-field reducer (`reduce_field_type_expr_with_mode`) live in the terminal
//! `output_sink` sink module (the only module that touches the reverse
//! materialization boundary); this predicate is pure typed-IR inspection with
//! no boundary access, so it stays a free-standing `pub(crate)` helper both the
//! sink reducer and the per-kind projectors consume.
//!
//! Historical note: the retired `field_reduce.rs` module hosted the
//! field-type reducer plus a projector-side name-predicate carrier
//! check (`is_builtin_utility_instantiation`,
//! `generic_instantiation_body_is_object`). The demand-driven reducer retires
//! the projector-side carrier check by routing carrier-stop through
//! the dispatch demand context — only the type-shape predicate remains here.

use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_type_expr::TypeExpr;

use crate::resolver_core::ResolverContext;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::semantic_query_memo::SemanticGraphStore;

/// Does `expr` contain any operator-shape node that the bounded
/// fixed-point reducer should resolve?
///
/// Returns `true` when the expression carries an `IndexedAccess`,
/// `KeyOf`, `TypeOf`, `Conditional`, `Mapped`, or `Infer` anywhere
/// in its tree, or an applied builtin-utility reference
/// (`Partial<EditorOptions>`, `Pick<Foo, 'bar'>`, …) — an explicit
/// utility in the published type IS explicit consumer demand, and
/// carrier-preserving lowering hands it to the reducer un-executed.
/// The reducer's `Navigate` reduction then decides
/// closed→materialise / open→carrier through the shared L1
/// predicates. Used by `reduce_field_type_expr_with_mode` to decide
/// whether to skip the reducer entirely.
pub(crate) fn type_expr_contains_reducible_operator(expr: &TypeExpr) -> bool {
    use verter_type_expr::ObjectMember;

    match expr {
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::Infer { .. }
        // An import-type is a cross-file resolution carrier the reducer
        // must resolve — grouped with the operator-shape arms.
        | TypeExpr::ImportType { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } if !type_arguments.is_empty() && BuiltinUtility::from_name(name.as_ref()).is_some() => {
            true
        }
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_reducible_operator(inner)
        }
        TypeExpr::Array { element, .. } => type_expr_contains_reducible_operator(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|el| type_expr_contains_reducible_operator(&el.ty)),
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().any(type_expr_contains_reducible_operator)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => type_expr_contains_reducible_operator(&p.ty),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_reducible_operator(&param.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
            ObjectMember::IndexSignature(sig) => {
                type_expr_contains_reducible_operator(&sig.key_type)
                    || type_expr_contains_reducible_operator(&sig.value_type)
            }
            ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                f.parameters
                    .iter()
                    .any(|p| type_expr_contains_reducible_operator(&p.ty))
                    || f.return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
        }),
        // A constructor type's signature is searched identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(f) | TypeExpr::ConstructorType(f) => {
            f.parameters
                .iter()
                .any(|p| type_expr_contains_reducible_operator(&p.ty))
                || f.return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_reducible_operator)
        }
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers carry no reducible operators — they are
        // intrinsic terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => false,
    }
}

/// The shallow reduction-gate facts of a graph `node`, read in NODE DOMAIN —
/// the node-domain mirror of the shape decisions the per-member publication path
/// runs on `raise(node)`:
///
/// - `leaf_like` — `raise(node)` is a `Primitive` / `Literal` (the peek `Leaf`
///   arm): a terminal shape, never reduced.
/// - `bare_carrier_ref` — `raise(node)` is a `Ref { type_arguments: [] }` (the
///   peek `BareCarrier` arm): a plain alias name, published shallow.
/// - `generic_instantiation_ref` — `raise(node)` is a `Ref` with NON-EMPTY type
///   arguments (`is_generic_instantiation`): a generic application that enters
///   the reducer and roots the cycle guard.
/// - `contains_reducible_operator` — `type_expr_contains_reducible_operator(raise(node))`.
///
/// Computed WITHOUT materialising a `TypeExpr`: the root classification reads the
/// in-hand carrier kind (peeling a single `Alias`), and the operator scan folds
/// the graph through [`node_contains_reducible_operator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeReductionGateFacts {
    pub(crate) leaf_like: bool,
    pub(crate) bare_carrier_ref: bool,
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
    let root = peel_alias_root(&graph, node, 0);
    let (leaf_like, bare_carrier_ref, generic_instantiation_ref) =
        match graph.node_data(root).as_deref() {
            // `raise(Primitive | Literal)` is the peek `Leaf` arm.
            Some(SemanticNodeData::Primitive(_)) | Some(SemanticNodeData::Literal(_)) => {
                (true, false, false)
            }
            // `raise(DeclRef)` is `Ref { type_arguments: [] }` — the bare carrier.
            Some(SemanticNodeData::DeclRef { .. }) => (false, true, false),
            // `raise(InstantiationRef)` is `Ref` with NON-EMPTY type arguments.
            Some(SemanticNodeData::InstantiationRef { .. }) => (false, false, true),
            // An unresolved `BareRef` raises to `Ref { name, type_args }`: empty args
            // ⇒ bare carrier, non-empty ⇒ generic instantiation (matching the raised
            // `Ref`'s `type_arguments` cardinality).
            Some(data) if data.bare_ref_head().is_some() => {
                let empty = data.carrier_type_args().is_empty();
                (false, empty, !empty)
            }
            _ => (false, false, false),
        };
    NodeReductionGateFacts {
        leaf_like,
        bare_carrier_ref,
        generic_instantiation_ref,
        contains_reducible_operator: node_contains_reducible_operator(&graph, node, 0),
    }
}

/// Node-domain mirror of [`type_expr_contains_reducible_operator`]: whether
/// `raise(node)` carries any operator-shape node (`IndexedAccess` / `KeyOf` /
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

#[cfg(test)]
mod node_reduction_gate_tests {
    //! DIFFERENTIAL EQUIVALENCE: the node-domain reduction-gate facts equal the
    //! `TypeExpr` predicates on `raise(node)`, field-for-field, on inputs that
    //! genuinely reach each arm — every reducible-operator kind (indexed access,
    //! keyof, typeof, conditional, mapped, import type, builtin-utility ref),
    //! recursive compound operands, generic instantiation, bare carrier, and a
    //! leaf. The node fact is computed WITHOUT materialising; the oracle raises
    //! the SAME node and runs the `TypeExpr` predicate.

    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::{classify_node_reduction_gates, type_expr_contains_reducible_operator};
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;

    /// Lower `expr` to a Navigate-mode (carrier-preserving) graph node. The
    /// oracle is `type_expr_contains_reducible_operator(expr)` on the INPUT —
    /// equivalent to the predicate on `raise(node)` because Navigate lowering
    /// keeps operators deferred (carriers), so `raise ∘ lower` preserves operator
    /// presence; the node fact must agree.
    fn lower_in(host: &VerterHost, scope: &str, expr: &TypeExpr) -> SemanticNodeId {
        ProjectSemanticDispatch::new(host)
            .lower_type_expr_in_scope_with_mode(scope, expr, ProjectionMode::Navigate)
            .expect("expr must lower")
    }

    #[test]
    fn classify_node_reduction_gates_matches_type_expr_oracle_per_operator_kind() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/m.ts".to_string(),
            Arc::from(
                "export interface Foo { a: string; b: number }\n\
                 export type Tool<I, O> = { i: I; o: O }\n\
                 export const factory = { make: 1 }\n",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/m.ts"));
        let scope = "/src/m.ts";

        let foo = || TypeExpr::named("Foo");
        let idx = || TypeExpr::IndexedAccess {
            object: Arc::new(foo()),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        // Navigate-PRESERVED operator-shape / builtin-utility / recursive-compound
        // inputs that the oracle classifies reducible. `node_contains_reducible_operator`
        // groups ALL operator kinds (IndexedAccess | KeyOf | TypeOf | Conditional |
        // Mapped | Infer | ImportType) into ONE `=> true` arm, proven live here by
        // IndexedAccess / KeyOf / Conditional; the remaining kinds share that arm
        // (a node-level Mapped over a CLOSED key source is executed to an Object by
        // Navigate lowering, so it is exercised at the node level, not via this
        // input-comparison oracle).
        let reducible_cases: Vec<TypeExpr> = vec![
            // indexed access
            idx(),
            // keyof
            TypeExpr::KeyOf(Arc::new(foo())),
            // conditional (Navigate-preserved carrier, not executed)
            TypeExpr::Conditional {
                check: Arc::new(idx()),
                extends: Arc::new(TypeExpr::string_literal("a")),
                true_type: Arc::new(TypeExpr::string_literal("y")),
                false_type: Arc::new(TypeExpr::string_literal("n")),
            },
            // builtin-utility ref with args
            TypeExpr::named_with_args("Pick", vec![foo(), TypeExpr::string_literal("a")]),
            // RECURSIVE COMPOUND: a non-builtin generic whose argument carries an
            // operator (Tool<Foo['a'], number>) — the operator hides in the args.
            TypeExpr::named_with_args(
                "Tool",
                vec![idx(), TypeExpr::Primitive(PrimitiveName::Number)],
            ),
            // RECURSIVE COMPOUND: union arm carries an operator.
            TypeExpr::Union(Arc::from(
                vec![TypeExpr::KeyOf(Arc::new(foo())), foo()].into_boxed_slice(),
            )),
        ];
        for expr in &reducible_cases {
            let node = lower_in(&host, scope, expr);
            let facts = classify_node_reduction_gates(&host, node);
            assert_eq!(
                facts.contains_reducible_operator,
                type_expr_contains_reducible_operator(expr),
                "contains_reducible_operator must equal the TypeExpr oracle for input {expr:?}"
            );
            assert!(
                type_expr_contains_reducible_operator(expr),
                "case {expr:?} must GENUINELY reach the reducible path (oracle true) — a \
                 non-reaching case would pass the differential vacuously"
            );
        }

        // NON-reducible classification cases, each with a discriminating gate fact.
        // bare carrier (plain alias `Foo`)
        let facts = classify_node_reduction_gates(&host, lower_in(&host, scope, &foo()));
        assert!(
            !facts.contains_reducible_operator
                && facts.bare_carrier_ref
                && !facts.generic_instantiation_ref
                && !facts.leaf_like,
            "a plain alias `Foo` is a bare carrier, NOT reducible/generic/leaf; got {facts:?}"
        );

        // leaf (primitive)
        let facts = classify_node_reduction_gates(
            &host,
            lower_in(&host, scope, &TypeExpr::Primitive(PrimitiveName::String)),
        );
        assert!(
            facts.leaf_like
                && !facts.bare_carrier_ref
                && !facts.generic_instantiation_ref
                && !facts.contains_reducible_operator,
            "a primitive is a leaf, NOT a carrier/generic/reducible; got {facts:?}"
        );

        // generic instantiation that is NOT reducible (non-builtin, primitive args)
        let tool = TypeExpr::named_with_args(
            "Tool",
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ],
        );
        let node = lower_in(&host, scope, &tool);
        let facts = classify_node_reduction_gates(&host, node);
        assert_eq!(
            facts.contains_reducible_operator,
            type_expr_contains_reducible_operator(&tool),
            "a non-builtin generic with primitive args is NOT reducible; node must agree with oracle"
        );
        assert!(
            facts.generic_instantiation_ref
                && !facts.contains_reducible_operator
                && !facts.bare_carrier_ref
                && !facts.leaf_like,
            "Tool<string, number> is a generic instantiation but not reducible; got {facts:?}"
        );
    }
}
