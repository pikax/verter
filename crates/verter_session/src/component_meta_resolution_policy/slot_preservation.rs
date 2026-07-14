//! Slot-binding indexed-access symbolic preservation.
//!
//! Force the slot binding's published source back to the symbolic
//! `IndexedAccess` shape encoded in the authored annotation source when the
//! indexed access transits through an imported declaration. The eager
//! evaluator may have widened the access through an open
//! `[k: string]: any` index signature; the navigable member-path contract
//! is the better public surface. All classification runs node-domain off
//! sources raised through the ONE shared dispatch.

use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};
use verter_type_expr::facts::SemanticTypeSource;

use super::core::{DeclLookup, PolicyCtx};

/// Whether the slot binding's authored annotation SOURCE describes an
/// indexed access that transits through an imported declaration. When
/// true, the caller restores the symbolic form from the authored source
/// and skips the expansion walk.
pub(super) fn slot_binding_should_preserve_symbolic_raw_type(
    raw_type_source: Option<&SemanticTypeSource>,
    ctx: &mut PolicyCtx<'_, '_>,
) -> bool {
    let Some(raw_source) = raw_type_source else {
        return false;
    };
    let Some(hot) = ctx.raise_source(raw_source) else {
        return false;
    };
    raw_indexed_access_root_is_imported(hot.node(), ctx)
}

/// Returns true when the raised node is an `IndexedAccess` whose deref
/// chain transits through a reference to an imported declaration. The
/// "indexed root" is the chain starting from the indexed access's `object`
/// and the property body that the access selects from the root's
/// declaration body.
fn raw_indexed_access_root_is_imported(node: SemanticNodeId, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    let Some(data) = ctx.node_data(node) else {
        return false;
    };
    let SemanticNodeData::IndexedAccess { object, index } = data.as_ref() else {
        return false;
    };
    let object = *object;
    // Index must be a string key — that is the member-path the policy can
    // statically inspect inside the root's declaration body.
    let member = match index {
        IndexKey::String(member) => member.to_string(),
        _ => return false,
    };
    // For the slot binding case we expect a reference to a declaration at
    // the object position.
    let Some((name, _)) = ctx.node_ref_head(object) else {
        return false;
    };
    let Some(DeclLookup {
        canonical_source: _,
        body,
    }) = ctx.locate_declaration(name.as_str())
    else {
        return false;
    };
    // The root's declaration body must raise to an Object whose `member`
    // property value contains an imported reference (or itself resolves to
    // an imported declaration). The root's own location is not the trigger.
    let Some(body_hot) = ctx.raise_source(&body) else {
        return false;
    };
    let property_value = match ctx.node_data(body_hot.node()).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface
            .members
            .iter()
            .find(|candidate| candidate.name.as_ref() == member)
            .map(|candidate| candidate.value),
        _ => None,
    };
    let Some(property_value) = property_value else {
        return false;
    };
    node_contains_imported_ref(property_value, ctx)
}

/// Walks the raised node graph and returns true on the first reference
/// whose declaration resolves to an imported (non-owner) declaration.
/// References whose declarations cannot be located are ignored — they
/// cannot be proven imported. A cross-file `import("…")` carrier is, by
/// construction, a reference to an imported declaration.
fn node_contains_imported_ref(root: SemanticNodeId, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![root];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) = ctx.node_ref_head(node) {
            if let Some(DeclLookup {
                canonical_source, ..
            }) = ctx.locate_declaration(name.as_str())
            {
                if canonical_source != ctx.owner_canonical {
                    return true;
                }
            }
            worklist.extend(args);
            continue;
        }
        let Some(data) = ctx.node_data(node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::ImportType(_) => return true,
            SemanticNodeData::Alias(target) => worklist.push(*target),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                worklist.extend(arms.iter().copied());
            }
            SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
                worklist.push(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                worklist.push(*object);
                if let IndexKey::TypeNode(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            SemanticNodeData::Object(surface) => {
                worklist.extend(surface.members.iter().map(|member| member.value));
                worklist.extend(surface.call_signatures.iter().copied());
                worklist.extend(surface.construct_signatures.iter().copied());
                for signature in surface.index_signatures.iter() {
                    worklist.push(signature.key_type);
                    worklist.push(signature.value_type);
                }
            }
            SemanticNodeData::Function {
                params,
                return_type,
                ..
            } => {
                worklist.extend(params.iter().map(|param| param.ty));
                worklist.push(*return_type);
            }
            SemanticNodeData::ConstructorType { signature } => worklist.push(*signature),
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                worklist.push(*check);
                worklist.push(*extends);
                worklist.push(*true_branch_ref);
                worklist.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, .. } => worklist.push(*source),
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                worklist.extend(expressions.iter().copied());
            }
            SemanticNodeData::MergedDecl { contributors } => {
                worklist.extend(contributors.iter().copied());
            }
            _ => {}
        }
    }
    false
}
