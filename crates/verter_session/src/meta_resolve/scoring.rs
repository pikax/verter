//! Symbolic-penalty + materialization-improvement scoring helpers.
//!
//! domain 10 — TypeExpr-walking helpers used by the materialization
//! pipeline to choose between candidate surfaces. These are pure leaf helpers:
//! no shared state, no host access, no graph access.

pub(crate) fn count_symbolic_carriers_in_expr(expr: &verter_type_expr::TypeExpr) -> usize {
    use verter_type_expr::{ObjectMember, TypeExpr};

    let mut score = 0usize;
    let mut stack = vec![expr];

    while let Some(current) = stack.pop() {
        match current {
            TypeExpr::Primitive(_) | TypeExpr::Literal(_) => {}
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => stack.push(inner),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter().rev() {
                    stack.push(&element.ty);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter().rev() {
                    stack.push(ty);
                }
            }
            TypeExpr::Object(object) => {
                for member in object.properties.iter().rev() {
                    match member {
                        ObjectMember::Property(property) => stack.push(&property.ty),
                        ObjectMember::Method(method) => {
                            if let Some(return_type) = method.function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in method.function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            stack.push(&signature.value_type);
                            stack.push(&signature.key_type);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            if let Some(return_type) = function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                    }
                }
            }
            // A constructor type carries the same `FunctionExpr` payload as a
            // function type; its signature is walked identically.
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
                if let Some(return_type) = function.return_type.as_deref() {
                    stack.push(return_type);
                }
                for parameter in function.parameters.iter().rev() {
                    stack.push(&parameter.ty);
                }
            }
            TypeExpr::Ref { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            // Mirrors the `Ref` arm: an import-type is a symbolic carrier
            // (count +1) and its nested `type_arguments` are recursed.
            TypeExpr::ImportType { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                score += 1;
                stack.push(index);
                stack.push(object);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                score += 1;
                stack.push(false_type);
                stack.push(true_type);
                stack.push(extends);
                stack.push(check);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                score += 1;
                if let Some(name_type) = name_type.as_deref() {
                    stack.push(name_type);
                }
                stack.push(value);
                stack.push(source);
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                score += 1;
                for expression in expressions.iter().rev() {
                    stack.push(expression);
                }
            }
            TypeExpr::RecursiveRef { type_arguments, .. } => {
                score += 1;
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_)
            // Synthetic slot-binding carrier IS itself a symbolic
            // carrier (an unresolved binding identity); count it as one.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => {
                score += 1;
            }
        }
    }

    score
}

fn count_generic_detail_in_expr(expr: &verter_type_expr::TypeExpr) -> usize {
    use verter_type_expr::{ObjectMember, TypeExpr};

    let mut score = 0usize;
    let mut stack = vec![expr];

    while let Some(current) = stack.pop() {
        match current {
            TypeExpr::TypeParameter(parameter) => {
                score += 1;
                if let Some(default) = parameter.default.as_deref() {
                    stack.push(default);
                }
                if let Some(constraint) = parameter.constraint.as_deref() {
                    stack.push(constraint);
                }
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => stack.push(inner),
            TypeExpr::Tuple { elements, .. } => {
                for element in elements.iter().rev() {
                    stack.push(&element.ty);
                }
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types.iter().rev() {
                    stack.push(ty);
                }
            }
            TypeExpr::Object(object) => {
                for member in object.properties.iter().rev() {
                    match member {
                        ObjectMember::Property(property) => stack.push(&property.ty),
                        ObjectMember::Method(method) => {
                            for type_parameter in method.function.type_parameters.iter().rev() {
                                score += 1;
                                if let Some(default) = type_parameter.default.as_deref() {
                                    stack.push(default);
                                }
                                if let Some(constraint) = type_parameter.constraint.as_deref() {
                                    stack.push(constraint);
                                }
                            }
                            if let Some(return_type) = method.function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in method.function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            stack.push(&signature.value_type);
                            stack.push(&signature.key_type);
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for type_parameter in function.type_parameters.iter().rev() {
                                score += 1;
                                if let Some(default) = type_parameter.default.as_deref() {
                                    stack.push(default);
                                }
                                if let Some(constraint) = type_parameter.constraint.as_deref() {
                                    stack.push(constraint);
                                }
                            }
                            if let Some(return_type) = function.return_type.as_deref() {
                                stack.push(return_type);
                            }
                            for parameter in function.parameters.iter().rev() {
                                stack.push(&parameter.ty);
                            }
                        }
                    }
                }
            }
            // A constructor type's signature (type-parameters, return,
            // parameters) is walked identically to a function type's.
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
                for type_parameter in function.type_parameters.iter().rev() {
                    score += 1;
                    if let Some(default) = type_parameter.default.as_deref() {
                        stack.push(default);
                    }
                    if let Some(constraint) = type_parameter.constraint.as_deref() {
                        stack.push(constraint);
                    }
                }
                if let Some(return_type) = function.return_type.as_deref() {
                    stack.push(return_type);
                }
                for parameter in function.parameters.iter().rev() {
                    stack.push(&parameter.ty);
                }
            }
            TypeExpr::Ref { type_arguments, .. }
            | TypeExpr::RecursiveRef { type_arguments, .. }
            // An import-type carries no generic detail of its own; like a
            // `Ref`, only its nested `type_arguments` are recursed.
            | TypeExpr::ImportType { type_arguments, .. } => {
                for argument in type_arguments.iter().rev() {
                    stack.push(argument);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                stack.push(index);
                stack.push(object);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                stack.push(false_type);
                stack.push(true_type);
                stack.push(extends);
                stack.push(check);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                if let Some(name_type) = name_type.as_deref() {
                    stack.push(name_type);
                }
                stack.push(value);
                stack.push(source);
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for expression in expressions.iter().rev() {
                    stack.push(expression);
                }
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            // Synthetic carriers carry no generic detail.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => {}
        }
    }

    score
}

fn type_expr_has_structural_top_level(expr: &verter_type_expr::TypeExpr) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => type_expr_has_structural_top_level(inner),
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Unknown { .. }
        // Synthetic carriers are intrinsic terminal carriers, not a
        // concrete structural shape — treat as non-structural.
        | TypeExpr::SyntheticSlotBinding(_)
        // An import-type is a symbolic cross-file reference (like a `Ref`),
        // not a concrete structural shape — non-structural.
        | TypeExpr::ImportType { .. }
        | TypeExpr::Infer { .. } => false,
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Object(_)
        | TypeExpr::Function(_)
        // A constructor type is a concrete structural shape, like a function.
        | TypeExpr::ConstructorType(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

// ===========================================================================
// Node-domain mirrors of the scoring helpers, computed on `raise(node)` WITHOUT
// materialising a `TypeExpr`. The publication-finaliser compares two candidate
// published-field NODES (the field's reduction vs the shallow form's reduction)
// in node domain and materialises only the winner. Parity-locked field-for-field
// against the `TypeExpr` scoring via a differential test.
// ===========================================================================

/// Node-domain mirror of [`count_symbolic_carriers_in_expr`]: the symbolic-carrier
/// penalty of `raise(node)`. A reference carrier (`DeclRef` / `InstantiationRef` /
/// `BareRef` — all raise to `Ref`), `ImportType`, `IndexedAccess`, `Conditional`,
/// `Mapped`, `TemplateLiteral`, and a `TypeOf` / `Opaque` / `Infer` leaf each cost
/// `+1`; `KeyOf` / `Alias` / compound shapes recurse without a self-penalty;
/// `Primitive` / `Literal` cost `0`. `depth` fuses at 256.
fn count_symbolic_carriers_in_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> usize {
    use crate::semantic_query::{IndexKey, SemanticNodeData};
    if depth > 256 {
        return 0;
    }
    let Some(data) = graph.node_data(node) else {
        return 0;
    };
    let recur = |n: crate::semantic_query::SemanticNodeId| {
        count_symbolic_carriers_in_node(graph, n, depth + 1)
    };
    match data.as_ref() {
        SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_) => 0,
        // Reference carriers raise to `Ref { type_arguments }` → +1 plus the args.
        SemanticNodeData::DeclRef { .. } => 1,
        SemanticNodeData::InstantiationRef { args, .. } => {
            1 + args.iter().map(|&a| recur(a)).sum::<usize>()
        }
        // `ImportType` raises to `TypeExpr::ImportType` (+1 + args); a `BareRef`
        // raises to `Ref` (+1 + args). Both descend `carrier_type_args`.
        SemanticNodeData::ImportType(_) => {
            1 + data
                .carrier_type_args()
                .iter()
                .map(|&a| recur(a))
                .sum::<usize>()
        }
        d if d.bare_ref_head().is_some() => {
            1 + d
                .carrier_type_args()
                .iter()
                .map(|&a| recur(a))
                .sum::<usize>()
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            let idx = match index {
                IndexKey::TypeNode(n) => recur(*n),
                _ => 0,
            };
            1 + recur(*object) + idx
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            1 + recur(*check) + recur(*extends) + recur(*true_branch_ref) + recur(*false_branch_ref)
        }
        SemanticNodeData::Mapped { source, mapper } => {
            let remap = mapper.name_remap.map_or(0, recur);
            1 + recur(*source) + recur(mapper.key_space) + recur(mapper.value_expr) + remap
        }
        SemanticNodeData::TemplateLiteral { expressions, .. } => {
            1 + expressions.iter().map(|&e| recur(e)).sum::<usize>()
        }
        // `TypeOf` / `Opaque` (raises to `Unknown`) / `Infer` are symbolic leaves.
        SemanticNodeData::TypeOf(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::Infer { .. } => 1,
        // `KeyOf` recurses without a self-penalty (the `TypeExpr` `KeyOf(inner)`
        // arm pushes `inner`, no `+1`).
        SemanticNodeData::KeyOf { base } => recur(*base),
        SemanticNodeData::Alias(inner) => recur(*inner),
        SemanticNodeData::Array { element, .. } => recur(*element),
        SemanticNodeData::Tuple { elements, .. } => elements.iter().map(|el| recur(el.value)).sum(),
        SemanticNodeData::Union(members)
        | SemanticNodeData::Intersection(members)
        | SemanticNodeData::MergedDecl {
            contributors: members,
        } => members.iter().map(|&m| recur(m)).sum(),
        SemanticNodeData::Object(surface) => {
            surface
                .members
                .iter()
                .map(|m| recur(m.value))
                .sum::<usize>()
                + surface
                    .index_signatures
                    .iter()
                    .map(|sig| recur(sig.key_type) + recur(sig.value_type))
                    .sum::<usize>()
                + surface
                    .call_signatures
                    .iter()
                    .map(|&c| recur(c))
                    .sum::<usize>()
                + surface
                    .construct_signatures
                    .iter()
                    .map(|&c| recur(c))
                    .sum::<usize>()
        }
        SemanticNodeData::Function {
            params,
            return_type,
            ..
        } => params.iter().map(|p| recur(p.ty)).sum::<usize>() + recur(*return_type),
        SemanticNodeData::ConstructorType { signature } => recur(*signature),
        _ => 0,
    }
}

/// Node-domain mirror of `count_generic_detail_in_expr`: the type-parameter detail
/// of `raise(node)`. A `Function`'s declared `type_parameters` each cost `+1`
/// (plus their constraint / default); reference carriers and compound shapes
/// recurse without a self cost. `depth` fuses at 256. (A standalone free type
/// parameter has no node variant — it only enters through a `Function`'s
/// `type_parameters`, which are counted here.)
fn count_generic_detail_in_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> usize {
    use crate::semantic_query::{IndexKey, SemanticNodeData};
    if depth > 256 {
        return 0;
    }
    let Some(data) = graph.node_data(node) else {
        return 0;
    };
    let recur = |n: crate::semantic_query::SemanticNodeId| {
        count_generic_detail_in_node(graph, n, depth + 1)
    };
    let type_params = |tps: &[crate::semantic_query::TypeParamDecl]| -> usize {
        tps.iter()
            .map(|tp| 1 + tp.constraint.map_or(0, recur) + tp.default.map_or(0, recur))
            .sum()
    };
    match data.as_ref() {
        SemanticNodeData::InstantiationRef { args, .. } => args.iter().map(|&a| recur(a)).sum(),
        SemanticNodeData::ImportType(_) => data.carrier_type_args().iter().map(|&a| recur(a)).sum(),
        d if d.bare_ref_head().is_some() => d.carrier_type_args().iter().map(|&a| recur(a)).sum(),
        SemanticNodeData::IndexedAccess { object, index } => {
            let idx = match index {
                IndexKey::TypeNode(n) => recur(*n),
                _ => 0,
            };
            recur(*object) + idx
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => recur(*check) + recur(*extends) + recur(*true_branch_ref) + recur(*false_branch_ref),
        SemanticNodeData::Mapped { source, mapper } => {
            let remap = mapper.name_remap.map_or(0, recur);
            recur(*source) + recur(mapper.key_space) + recur(mapper.value_expr) + remap
        }
        SemanticNodeData::KeyOf { base } => recur(*base),
        SemanticNodeData::Alias(inner) => recur(*inner),
        SemanticNodeData::Array { element, .. } => recur(*element),
        SemanticNodeData::Tuple { elements, .. } => elements.iter().map(|el| recur(el.value)).sum(),
        SemanticNodeData::Union(members)
        | SemanticNodeData::Intersection(members)
        | SemanticNodeData::MergedDecl {
            contributors: members,
        } => members.iter().map(|&m| recur(m)).sum(),
        SemanticNodeData::Object(surface) => {
            surface
                .members
                .iter()
                .map(|m| recur(m.value))
                .sum::<usize>()
                + surface
                    .index_signatures
                    .iter()
                    .map(|sig| recur(sig.key_type) + recur(sig.value_type))
                    .sum::<usize>()
                + surface
                    .call_signatures
                    .iter()
                    .map(|&c| recur(c))
                    .sum::<usize>()
                + surface
                    .construct_signatures
                    .iter()
                    .map(|&c| recur(c))
                    .sum::<usize>()
        }
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            ..
        } => {
            type_params(type_parameters)
                + params.iter().map(|p| recur(p.ty)).sum::<usize>()
                + recur(*return_type)
        }
        SemanticNodeData::ConstructorType { signature } => recur(*signature),
        SemanticNodeData::TemplateLiteral { expressions, .. } => {
            expressions.iter().map(|&e| recur(e)).sum()
        }
        _ => 0,
    }
}

/// Node-domain mirror of `type_expr_has_structural_top_level`: whether
/// `raise(node)`'s ROOT is a concrete structural shape (NOT a symbolic reference /
/// operator carrier). Reference carriers, `IndexedAccess`, `Conditional`,
/// `Mapped`, `TypeOf`, `Opaque`, `SyntheticSlotBinding`, `ImportType`, `Infer` are
/// non-structural; `Primitive` / `Literal` / `Object` / `Function` /
/// `ConstructorType` / `Array` / `Tuple` / `Union` / `Intersection` / `KeyOf` /
/// `TemplateLiteral` / `MergedDecl` are structural; `Alias` peels through.
fn node_has_structural_top_level(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => node_has_structural_top_level(graph, *inner),
        SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Object(_)
        | SemanticNodeData::Function { .. }
        | SemanticNodeData::ConstructorType { .. }
        | SemanticNodeData::Array { .. }
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::Union(_)
        | SemanticNodeData::Intersection(_)
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::MergedDecl { .. } => true,
        _ => false,
    }
}

/// Whether `raise(node)`'s ROOT is an unmaterialised-miss sentinel — the
/// node-domain mirror of `matches!(current, TypeExpr::Unknown { .. })` the
/// scoring's first clause tests. A graph `Opaque` carrier raises to
/// `TypeExpr::Unknown`.
fn node_root_is_unknown(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Alias(inner)) => node_root_is_unknown(graph, *inner),
        Some(SemanticNodeData::Opaque(_)) => true,
        _ => false,
    }
}

/// Node-domain mirror of [`compare_type_expr_improvement`]: whether `candidate`
/// is a strictly BETTER published shape than `current`, scored on `raise(*)`
/// without materialising. Used by the publication finaliser to pick between the
/// field's reduction and the shallow form's reduction in node domain.
pub(crate) fn compare_node_improvement(
    ctx: &dyn crate::resolver_core::ResolverContext,
    candidate: crate::semantic_query::SemanticNodeId,
    current: crate::semantic_query::SemanticNodeId,
) -> bool {
    let graph = ctx.project_type_store().semantic_graph();
    if node_root_is_unknown(graph, current) && !node_root_is_unknown(graph, candidate) {
        return true;
    }
    let candidate_score = count_symbolic_carriers_in_node(graph, candidate, 0);
    let current_score = count_symbolic_carriers_in_node(graph, current, 0);
    candidate_score < current_score
        || (node_has_structural_top_level(graph, candidate)
            && !node_has_structural_top_level(graph, current))
        || (candidate_score == current_score
            && count_generic_detail_in_node(graph, candidate, 0)
                > count_generic_detail_in_node(graph, current, 0))
}

/// Node-domain mirror of the publication finaliser's
/// `root_is_explicit_selector_operator`: whether `raise(node)`'s ROOT is an
/// explicit consumer-demand selector (`IndexedAccess` / `keyof` / `typeof` / a
/// `Pick` / `Omit` / `Record` builtin-utility reference). Reads the carrier kind
/// directly (peeling `Alias`); a builtin-utility name is matched on the
/// reference's declaration name.
pub(crate) fn node_root_is_explicit_selector_operator(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
    let graph = ctx.project_type_store().semantic_graph();
    let is_selector_util = |name: &str| {
        matches!(
            BuiltinUtility::from_name(name),
            Some(BuiltinUtility::Pick | BuiltinUtility::Omit | BuiltinUtility::Record)
        )
    };
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Alias(inner)) => {
            node_root_is_explicit_selector_operator(ctx, *inner)
        }
        Some(
            SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::TypeOf(_),
        ) => true,
        Some(SemanticNodeData::DeclRef { identity }) => {
            is_selector_util(identity.decl_name.as_ref())
        }
        Some(SemanticNodeData::InstantiationRef { base, .. }) => {
            is_selector_util(base.decl_name.as_ref())
        }
        Some(data) if data.bare_ref_head().is_some() => data
            .bare_ref_head()
            .is_some_and(|head| is_selector_util(head.0.as_ref())),
        _ => false,
    }
}

pub(crate) fn compare_type_expr_improvement(
    candidate: &verter_type_expr::TypeExpr,
    current: &verter_type_expr::TypeExpr,
) -> bool {
    if matches!(current, verter_type_expr::TypeExpr::Unknown { .. })
        && !matches!(candidate, verter_type_expr::TypeExpr::Unknown { .. })
    {
        return true;
    }

    let candidate_score = count_symbolic_carriers_in_expr(candidate);
    let current_score = count_symbolic_carriers_in_expr(current);

    candidate_score < current_score
        || (type_expr_has_structural_top_level(candidate)
            && !type_expr_has_structural_top_level(current))
        || (candidate_score == current_score
            && count_generic_detail_in_expr(candidate) > count_generic_detail_in_expr(current))
}

// promoted from `pub(super)` to `pub(crate)` so the moved
// `host_manage::component_meta_methods.rs` (formerly the in-tree
// `host_methods.rs`) reaches the function via the
// `crate::meta_resolve::component_meta_registry_prefers_structural_materialization`
// re-export. Without the promotion, the parent shell's `pub(crate) use`
// would be rejected (E0364).
pub(crate) fn component_meta_registry_prefers_structural_materialization(
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(_)
        | TypeExpr::Array { .. }
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Function(_)
        // A constructor type prefers structural materialization like a function.
        | TypeExpr::ConstructorType(_)
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_) => true,
        TypeExpr::Ref { .. }
        | TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers are intrinsic terminals — already final at
        // the projector surface; no structural materialisation needed.
        | TypeExpr::SyntheticSlotBinding(_)
        // An import-type is a symbolic cross-file reference (like a `Ref`);
        // it does not prefer structural materialisation.
        | TypeExpr::ImportType { .. }
        | TypeExpr::Infer { .. } => false,
    }
}

#[cfg(test)]
mod node_scoring_differential_tests {
    //! DIFFERENTIAL EQUIVALENCE: the node-domain scoring comparator equals the
    //! `TypeExpr` comparator (`compare_type_expr_improvement`) on Navigate-lowered
    //! nodes, exercising each scoring clause — current-Unknown, fewer symbolic
    //! carriers, structural-top-level, and a negative — plus the explicit-selector
    //! root predicate over its operator / builtin-utility kinds.

    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::{
        compare_node_improvement, compare_type_expr_improvement,
        node_root_is_explicit_selector_operator,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;

    fn build_host() -> VerterHost {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/m.ts".to_string(),
            Arc::from("export interface Foo { a: string; b: number }\n"),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/m.ts"));
        host
    }

    fn lower(host: &VerterHost, expr: &TypeExpr) -> SemanticNodeId {
        ProjectSemanticDispatch::new(host)
            .lower_type_expr_in_scope_with_mode("/src/m.ts", expr, ProjectionMode::Navigate)
            .expect("expr must lower")
    }

    #[test]
    fn compare_node_improvement_matches_type_expr_comparator_per_clause() {
        let host = build_host();
        let foo = || TypeExpr::named("Foo");
        let idx = || TypeExpr::IndexedAccess {
            object: Arc::new(foo()),
            index: Arc::new(TypeExpr::string_literal("a")),
        };
        // A structural array `string[]` (structural top-level, 0 symbolic carriers).
        let array = || TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        };

        // (candidate, current) pairs, one per scoring clause + a negative control.
        let pairs: Vec<(TypeExpr, TypeExpr)> = vec![
            // current-Unknown clause: a concrete string beats Unknown.
            (
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Unknown { raw: String::new() },
            ),
            // fewer-symbolic-carriers clause: a bare `Foo` (1) beats `Pick<Foo,'a'>` (2).
            (
                foo(),
                TypeExpr::named_with_args("Pick", vec![foo(), TypeExpr::string_literal("a")]),
            ),
            // structural-top-level clause: a structural array beats a Ref carrier.
            (array(), foo()),
            // NEGATIVE control: an IndexedAccess does NOT beat a bare string.
            (idx(), TypeExpr::Primitive(PrimitiveName::String)),
        ];

        let mut saw_true = false;
        let mut saw_false = false;
        for (candidate, current) in &pairs {
            let cand_node = lower(&host, candidate);
            let cur_node = lower(&host, current);
            let node_verdict = compare_node_improvement(&host, cand_node, cur_node);
            let expr_verdict = compare_type_expr_improvement(candidate, current);
            assert_eq!(
                node_verdict, expr_verdict,
                "compare_node_improvement must equal compare_type_expr_improvement for \
                 candidate={candidate:?} current={current:?}"
            );
            if node_verdict {
                saw_true = true;
            } else {
                saw_false = true;
            }
        }
        assert!(
            saw_true && saw_false,
            "the differential must exercise BOTH a better and a not-better verdict (genuine reach)"
        );
    }

    #[test]
    fn node_root_explicit_selector_matches_expected_kinds() {
        let host = build_host();
        let foo = || TypeExpr::named("Foo");
        // selectors → true
        for expr in [
            TypeExpr::IndexedAccess {
                object: Arc::new(foo()),
                index: Arc::new(TypeExpr::string_literal("a")),
            },
            TypeExpr::KeyOf(Arc::new(foo())),
            TypeExpr::named_with_args("Pick", vec![foo(), TypeExpr::string_literal("a")]),
            TypeExpr::named_with_args("Omit", vec![foo(), TypeExpr::string_literal("a")]),
        ] {
            let node = lower(&host, &expr);
            assert!(
                node_root_is_explicit_selector_operator(&host, node),
                "{expr:?} is an explicit selector operator at its root"
            );
        }
        // non-selectors → false
        for expr in [foo(), TypeExpr::Primitive(PrimitiveName::String)] {
            let node = lower(&host, &expr);
            assert!(
                !node_root_is_explicit_selector_operator(&host, node),
                "{expr:?} is NOT an explicit selector operator"
            );
        }
    }
}
