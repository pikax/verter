//! Node-domain registry object-surface classification.

use rustc_hash::FxHashSet;

use crate::semantic_query::SemanticNodeId;

/// Whether any reachable alias/union/intersection arm carries an explicit
/// object surface.
///
/// This is deliberately an existential arm-set predicate, not a normalized
/// root-shape predicate: an object arm still counts when an intersection fold
/// would drop it. The interned graph is acyclic, while the visited set dedupes
/// shared subgraphs and permits arbitrarily deep alias chains.
pub(crate) fn component_meta_registry_node_has_explicit_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;

    let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Object(_) | SemanticNodeData::MergedDecl { .. } => return true,
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                stack.extend(arms.iter().copied());
            }
            _ => {}
        }
    }
    false
}
