//! Restore symbolic macro-participating refs from the typed
//! source-annotation when the evaluator has eagerly resolved them away.
//!
//! The deleted `imported_props_like_public_raw_type` helper used the raw
//! type annotation as the canonical form for *Props imports. The
//! contract is re-instated here BEFORE the rule walk on prop / model /
//! accepted_prop type expressions, but classification is now structural
//! (§3.4 Typed-IR-Only Resolver Rule): "Props-like" means "consumed by
//! one of the owner's `defineProps` / `defineEmits` / `defineModel` /
//! `defineSlots` / `withDefaults` macros", NOT "identifier ends in
//! `Props`".

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use super::core::PolicyCtx;

/// If the user's source-annotation typed form contains imported
/// macro-participating refs that the evaluator eagerly resolved into
/// structural shapes (e.g. `ButtonProps[]` became `Array<Object{href,
/// disabled, label}>`), restore the symbolic form by inspecting the
/// typed annotation directly. The analyzer has already lowered the
/// source annotation via `lower_ts_type`; this helper walks the typed
/// form and never reparses text.
///
/// "Macro-participating" is structural — see §3.4. The set of
/// participating identities is built once in
/// `apply_component_meta_resolution_policy` and threaded via
/// `PolicyCtx::macro_participating_idents`.
///
/// **Only fires for COMPOUND raw types** — bare `Ref(macro-participating)`
/// raw types are left to the upstream `merge_evaluated_prop_types_into_meta`
/// policy (which already has the bare-Ref escape hatch at
/// host_manage.rs ~8170). Restoring bare Refs here would over-correct
/// cases like `avatar: AvatarProps` where the evaluator's substituted
/// Object body is the intended public shape (see
/// `resolve_component_meta_publishes_transitive_registry_aliases_for_nested_indexed_access_refs`).
///
/// Returns `true` if the type_expr was rewritten.
pub(super) fn restore_props_suffix_from_raw(
    type_expr: &mut TypeExpr,
    raw_type_expr: Option<&TypeExpr>,
    ctx: &mut PolicyCtx<'_, '_>,
) -> bool {
    let Some(parsed) = raw_type_expr else {
        return false;
    };

    // Bare macro-participating Refs stay deferred to the bare-Ref merge
    // escape hatch — see doc comment.
    if is_bare_macro_participating_ref(parsed, ctx) {
        return false;
    }

    let mut participating_refs: Vec<(Arc<str>, usize)> = Vec::new();
    collect_macro_participating_refs(parsed, ctx, &mut participating_refs);
    if participating_refs.is_empty() {
        return false;
    }

    // Confirm every collected macro-participating ref in the raw type
    // belongs to an imported declaration (project-local OR
    // package-backed). If any ref resolves to the owner itself, we
    // don't substitute — the eager resolution there is correct.
    for (name, _) in participating_refs.iter() {
        let lookup = ctx.locate_declaration(name.as_ref());
        let imported = lookup
            .as_ref()
            .map(|d| d.canonical_source != ctx.owner_canonical)
            .unwrap_or(false);
        if !imported {
            return false;
        }
    }

    // If the resolved type_expr already contains all of the
    // macro-participating refs, nothing to restore — the evaluator
    // preserved the symbolic form.
    let all_present = participating_refs
        .iter()
        .all(|(name, arity)| expr_contains_ref(type_expr, name.as_ref(), *arity));
    if all_present {
        return false;
    }

    *type_expr = parsed.clone();
    true
}

/// `Ref { name }` directly (optionally wrapped in `Parenthesized`)
/// whose name resolves to a macro-participating root identity.
fn is_bare_macro_participating_ref(expr: &TypeExpr, ctx: &PolicyCtx<'_, '_>) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => is_bare_macro_participating_ref(inner, ctx),
        TypeExpr::Ref { name, .. } => ctx.is_macro_participating(name.as_ref()),
        _ => false,
    }
}

/// Collect every `Ref { name, type_arguments }` pair where `name`
/// resolves to one of the owner's macro-participating root identities.
/// Tracks both name and type-argument arity to disambiguate generic
/// vs. non-generic forms.
fn collect_macro_participating_refs(
    expr: &TypeExpr,
    ctx: &PolicyCtx<'_, '_>,
    out: &mut Vec<(Arc<str>, usize)>,
) {
    match expr {
        TypeExpr::Parenthesized(inner) => collect_macro_participating_refs(inner, ctx, out),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if ctx.is_macro_participating(name.as_ref()) {
                let entry = (name.clone(), type_arguments.len());
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            for arg in type_arguments.iter() {
                collect_macro_participating_refs(arg, ctx, out);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_macro_participating_refs(ty, ctx, out);
            }
        }
        TypeExpr::Array { element, .. } => collect_macro_participating_refs(element, ctx, out),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_macro_participating_refs(&element.ty, ctx, out);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_macro_participating_refs(object, ctx, out);
            collect_macro_participating_refs(index, ctx, out);
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
