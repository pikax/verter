//! Graph-only reference-carrier reads shared by the dispatch and its
//! publication consumers: the reference HEAD of a carrier node and the ONE
//! bounded, visited-guarded walk over the reference carriers reachable from a
//! node. Both read only interned node data; neither resolves anything.

use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};

/// Unwrap ONE `Alias` hop (the node-domain analog of stripping a
/// `Parenthesized` wrapper).
fn unalias_one_hop<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &super::ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> SemanticNodeId {
    match super::node_data_for(dispatch.graph(), node).as_deref() {
        Some(SemanticNodeData::Alias(target)) => *target,
        _ => node,
    }
}

/// Every distinct NOMINAL `typeof` carrier reachable from `node` whose
/// declaring identity lives in `canonical`, in visit order.
///
/// Shares [`walk_reference_carriers`] with the registry's dependency-name
/// collection by
/// construction, so the two answers cannot disagree: a carrier whose head
/// name a rendered surface must have in scope is exactly a carrier this
/// returns, and a shape the walk does not descend contributes neither a
/// name nor a carrier.
pub(crate) fn collect_owner_local_nominal_carriers<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    canonical: &str,
    carriers: &mut Vec<SemanticNodeId>,
) {
    walk_reference_carriers(dispatch, node, &mut |reached, data, _| {
        if data
            .typeof_nominal_identity()
            .is_some_and(|identity| identity.canonical_id.as_ref() == canonical)
            && !carriers.contains(&reached)
        {
            carriers.push(reached);
        }
    });
}

/// The ONE bounded, visited-guarded walk over the reference carriers
/// reachable from `node`. `visit` sees every reached node together with its
/// reference-head name when it has one; descent stays the walk's own
/// decision, so two consumers can never disagree about what the rendered
/// surface reaches.
pub fn walk_reference_carriers<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    visit: &mut dyn FnMut(SemanticNodeId, &SemanticNodeData, Option<&str>),
) {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![node];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(data) = super::node_data_for(dispatch.graph(), node) else {
            continue;
        };
        if let Some((name, args)) = reference_carrier_head(dispatch, node) {
            visit(node, data.as_ref(), Some(name.as_str()));
            worklist.extend(args);
            continue;
        }
        visit(node, data.as_ref(), None);
        match data.as_ref() {
            SemanticNodeData::Alias(target) => worklist.push(*target),
            SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
                worklist.push(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let arms = composite.composite_members().expect("composite arm");
                worklist.extend(arms.iter().copied());
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                worklist.extend(expressions.iter().copied());
            }
            SemanticNodeData::Object(surface) => {
                worklist.extend(surface.positive_members().iter().map(|member| member.value));
                worklist.extend(surface.call_signatures.iter().copied());
                worklist.extend(surface.construct_signatures.iter().copied());
                for signature in surface.index_signatures.iter() {
                    worklist.push(signature.key_type);
                    worklist.push(signature.value_type);
                }
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                predicate,
                ..
            } => {
                worklist.extend(params.iter().map(|param| param.ty));
                worklist.push(*return_type);
                worklist.extend(predicate.and_then(|predicate| predicate.ty));
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                worklist.push(*object);
                if let IndexKey::Computed(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                pending,
                ..
            } => {
                if let Some(frame) = pending {
                    worklist.extend(frame.argument_nodes());
                }
                worklist.push(*check);
                worklist.push(*extends);
                worklist.push(*true_branch_ref);
                worklist.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, .. } => worklist.push(*source),
            SemanticNodeData::MergedDecl { contributors } => {
                worklist.extend(contributors.iter().copied());
            }
            // A `typeof` carrier's own head is not a further node — the
            // visitor records what a rendered surface needs from it — but
            // its type arguments are, so the walk descends into them.
            typeof_carrier @ (SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_)) => {
                worklist.extend(typeof_carrier.carrier_type_args().iter().copied());
            }
            _ => {}
        }
    }
}

/// The node's reference HEAD: `(name, type-argument nodes)` for the three
/// reference carriers (`BareRef` / `InstantiationRef` / `DeclRef`).
pub fn reference_carrier_head<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> Option<(String, Vec<SemanticNodeId>)> {
    let data = super::node_data_for(dispatch.graph(), unalias_one_hop(dispatch, node))?;
    if let Some((name, _scope)) = data.bare_ref_head() {
        return Some((name.to_string(), data.carrier_type_args().to_vec()));
    }
    match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => {
            Some((identity.decl_name.to_string(), Vec::new()))
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            Some((base.decl_name.to_string(), args.to_vec()))
        }
        _ => None,
    }
}
