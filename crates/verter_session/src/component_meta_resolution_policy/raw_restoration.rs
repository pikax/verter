//! Restore symbolic *Props refs from the raw type annotation when the
//! evaluator has eagerly resolved them away.
//!
//! The deleted `imported_props_like_public_raw_type` helper used the raw
//! type annotation as the canonical form for *Props imports — re-instate
//! that contract before the rule walk on prop / model / accepted_prop
//! type expressions.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use super::core::{is_props_suffix, PolicyCtx};

/// If the raw type annotation contains imported *Props refs that the
/// evaluator eagerly resolved into structural shapes (e.g. `ButtonProps[]`
/// became `Array<Object{href, disabled, label}>`), restore the symbolic
/// form by parsing the raw type and confirming the parsed shape matches.
///
/// **Only fires for COMPOUND raw types** — bare `Ref(*Props)` raw types
/// are left to the upstream `merge_evaluated_prop_types_into_meta` policy
/// (which already has the bare-Ref escape hatch at host_manage.rs ~8170).
/// Restoring bare Refs here would over-correct cases like `avatar:
/// AvatarProps` where the evaluator's substituted Object body is the
/// intended public shape (see
/// `resolve_component_meta_publishes_transitive_registry_aliases_for_nested_indexed_access_refs`).
///
/// Returns `true` if the type_expr was rewritten.
pub(super) fn restore_props_suffix_from_raw(
    type_expr: &mut TypeExpr,
    raw_type: Option<&str>,
    ctx: &mut PolicyCtx<'_, '_>,
) -> bool {
    let Some(raw) = raw_type else {
        return false;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    let parsed = verter_type_expr_oxc::parse_type_annotation(trimmed);

    // Bare Props-suffix Refs stay deferred to the bare-Ref merge escape
    // hatch — see doc comment.
    if is_bare_props_suffix_ref(&parsed) {
        return false;
    }

    let mut props_refs: Vec<(Arc<str>, usize)> = Vec::new();
    collect_props_suffix_refs(&parsed, &mut props_refs);
    if props_refs.is_empty() {
        return false;
    }

    // Confirm every collected *Props ref in the raw type belongs to an
    // imported declaration (project-local OR package-backed). If any ref
    // resolves to the owner itself, we don't substitute — the eager
    // resolution there is correct.
    for (name, _) in props_refs.iter() {
        let lookup = ctx.locate_declaration(name.as_ref());
        let imported = lookup
            .as_ref()
            .map(|d| d.canonical_source != ctx.owner_canonical)
            .unwrap_or(false);
        if !imported {
            return false;
        }
    }

    // If the resolved type_expr already contains all of the *Props refs,
    // nothing to restore — the evaluator preserved the symbolic form.
    let all_present = props_refs
        .iter()
        .all(|(name, arity)| expr_contains_ref(type_expr, name.as_ref(), *arity));
    if all_present {
        return false;
    }

    *type_expr = parsed;
    true
}

/// `Ref { name: "*Props" }` directly, optionally wrapped in `Parenthesized`.
fn is_bare_props_suffix_ref(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => is_bare_props_suffix_ref(inner),
        TypeExpr::Ref { name, .. } => is_props_suffix(name.as_ref()),
        _ => false,
    }
}

/// Collect every `Ref { name, type_arguments }` pair where `name` ends in
/// `"Props"`. Tracks both name and type-argument arity to disambiguate
/// generic vs. non-generic forms.
fn collect_props_suffix_refs(expr: &TypeExpr, out: &mut Vec<(Arc<str>, usize)>) {
    match expr {
        TypeExpr::Parenthesized(inner) => collect_props_suffix_refs(inner, out),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if is_props_suffix(name.as_ref()) {
                let entry = (name.clone(), type_arguments.len());
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            for arg in type_arguments.iter() {
                collect_props_suffix_refs(arg, out);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_props_suffix_refs(ty, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_props_suffix_refs(element, out),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_props_suffix_refs(&element.ty, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_props_suffix_refs(object, out);
            collect_props_suffix_refs(index, out);
        }
        _ => {}
    }
}

/// Whether `expr` contains a `Ref { name, type_arguments }` where
/// `name == target` AND `type_arguments.len() == arity`.
fn expr_contains_ref(expr: &TypeExpr, target: &str, arity: usize) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => expr_contains_ref(inner, target, arity),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            (name.as_ref() == target && type_arguments.len() == arity)
                || type_arguments
                    .iter()
                    .any(|arg| expr_contains_ref(arg, target, arity))
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(|ty| expr_contains_ref(ty, target, arity))
        }
        TypeExpr::Array { element, .. } => expr_contains_ref(element, target, arity),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| expr_contains_ref(&element.ty, target, arity)),
        TypeExpr::IndexedAccess { object, index } => {
            expr_contains_ref(object, target, arity) || expr_contains_ref(index, target, arity)
        }
        _ => false,
    }
}
