//! Restore symbolic macro-participating references from the authored
//! source-annotation when the evaluator has eagerly resolved them away.
//!
//! The authored annotation SOURCE is the canonical form for
//! macro-participating imports. The contract runs BEFORE the rule walk on
//! prop / binding / accepted-prop published sources; classification is
//! structural (§3.4 Typed-IR-Only Resolver Rule): "role-bearing" means
//! "consumed by one of the owner's `defineProps` / `defineEmits` /
//! `defineModel` / `defineSlots` / `withDefaults` macros", NOT "identifier
//! ends in `Props`".

use verter_type_expr::facts::SemanticTypeSource;

use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};

use super::core::PolicyCtx;

/// If the user's authored annotation SOURCE contains imported
/// macro-participating references that the evaluator eagerly resolved into
/// structural shapes (e.g. `ButtonProps[]` became `Array<Object{href,
/// disabled, label}>`), restore the symbolic form by publishing the
/// authored source. Both sides raise through the ONE shared dispatch and
/// classify node-domain; no text is ever reparsed.
///
/// "Macro-participating" is structural — see §3.4. The set of
/// participating identities is built once in
/// `apply_component_meta_resolution_policy` and threaded via
/// `PolicyCtx::macro_participating_idents`.
///
/// **Only fires for COMPOUND raw shapes** — a bare
/// `Ref(macro-participating)` raw annotation needs no restoration: the
/// normalized macro rows publish the shallow reference carrier directly
/// (shallow-by-default). Restoring bare references here would
/// over-correct cases like `avatar: AvatarProps` where the evaluator's
/// substituted Object body is the intended public shape.
///
/// Returns `true` if the published source was replaced.
pub(super) fn restore_props_suffix_from_raw(
    type_source: &mut verter_type_expr::facts::SourcePosition,
    raw_type_source: Option<&SemanticTypeSource>,
    ctx: &mut PolicyCtx<'_, '_>,
) -> bool {
    let Some(raw_source) = raw_type_source else {
        return false;
    };
    let Some(raw_hot) = ctx.raise_source(raw_source) else {
        return false;
    };
    let raw_node = raw_hot.node();

    // Bare macro-participating references stay deferred to the bare-Ref
    // merge escape hatch — see doc comment.
    if is_bare_macro_participating_ref(raw_node, ctx) {
        return false;
    }

    let mut participating_refs: Vec<(String, usize)> = Vec::new();
    collect_macro_participating_refs(raw_node, ctx, &mut participating_refs);
    if participating_refs.is_empty() {
        return false;
    }

    // Confirm every collected macro-participating reference in the raw
    // shape belongs to an imported declaration (project-local OR
    // package-backed). If any reference resolves to the owner itself, we
    // don't substitute — the eager resolution there is correct.
    for (name, _) in participating_refs.iter() {
        let lookup = ctx.locate_declaration(name.as_str());
        let imported = lookup
            .as_ref()
            .map(|d| d.canonical_source != ctx.owner_canonical)
            .unwrap_or(false);
        if !imported {
            return false;
        }
    }

    // If the resolved published source already contains all of the
    // macro-participating references, nothing to restore — the evaluator
    // preserved the symbolic form.
    let all_present = type_source
        .present()
        .and_then(|source| ctx.raise_source(source))
        .is_some_and(|hot| {
            participating_refs
                .iter()
                .all(|(name, arity)| node_contains_ref(hot.node(), name.as_str(), *arity, ctx))
        });
    if all_present {
        return false;
    }

    // Restoring publishes the AUTHOR'S OWN annotation source — a genuine
    // authored success position regardless of the prior state.
    *type_source = verter_type_expr::facts::SourcePosition::Present(raw_source.clone());
    true
}

/// A reference head directly at the raw root (unwrapping one alias hop)
/// whose name resolves to a macro-participating root identity.
fn is_bare_macro_participating_ref(node: SemanticNodeId, ctx: &PolicyCtx<'_, '_>) -> bool {
    if let Some((name, _)) = ctx.node_ref_head(node) {
        return ctx.is_macro_participating(name.as_str());
    }
    match ctx.node_data(node).as_deref() {
        Some(SemanticNodeData::Alias(target)) => is_bare_macro_participating_ref(*target, ctx),
        _ => false,
    }
}

/// Collect every reference head `(name, type-argument arity)` pair where
/// `name` resolves to one of the owner's macro-participating root
/// identities. Tracks both name and type-argument arity to disambiguate
/// generic vs. non-generic forms. Walks the raw node's composition spine
/// (unions / intersections / arrays / tuples / indexed-access arms /
/// reference args) — visited-guarded, since raised nodes may be shared.
fn collect_macro_participating_refs(
    root: SemanticNodeId,
    ctx: &PolicyCtx<'_, '_>,
    out: &mut Vec<(String, usize)>,
) {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![root];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) = ctx.node_ref_head(node) {
            if ctx.is_macro_participating(name.as_str()) {
                let entry = (name, args.len());
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            worklist.extend(args);
            continue;
        }
        match ctx.node_data(node).as_deref() {
            Some(SemanticNodeData::Alias(target)) => worklist.push(*target),
            Some(SemanticNodeData::Union(arms)) | Some(SemanticNodeData::Intersection(arms)) => {
                worklist.extend(arms.iter().copied());
            }
            Some(SemanticNodeData::Array { element, .. }) => worklist.push(*element),
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            Some(SemanticNodeData::IndexedAccess { object, index }) => {
                worklist.push(*object);
                if let IndexKey::TypeNode(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            _ => {}
        }
    }
}

/// Whether the raised node contains a reference head with `name == target`
/// AND the given type-argument arity, anywhere on the composition spine.
fn node_contains_ref(
    root: SemanticNodeId,
    target: &str,
    arity: usize,
    ctx: &PolicyCtx<'_, '_>,
) -> bool {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![root];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) = ctx.node_ref_head(node) {
            if name == target && args.len() == arity {
                return true;
            }
            worklist.extend(args);
            continue;
        }
        match ctx.node_data(node).as_deref() {
            Some(SemanticNodeData::Alias(target_node)) => worklist.push(*target_node),
            Some(SemanticNodeData::Union(arms)) | Some(SemanticNodeData::Intersection(arms)) => {
                worklist.extend(arms.iter().copied());
            }
            Some(SemanticNodeData::Array { element, .. }) => worklist.push(*element),
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            Some(SemanticNodeData::IndexedAccess { object, index }) => {
                worklist.push(*object);
                if let IndexKey::TypeNode(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            _ => {}
        }
    }
    false
}
