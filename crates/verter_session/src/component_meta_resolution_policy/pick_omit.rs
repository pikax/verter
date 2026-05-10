//! Selective `Pick<T, K>` / symbolic `Omit<T, K>` handling for
//! package-backed declarations.
//!
//! When the target declaration's canonical source resolves under
//! a package-backed path, the helper module owns the materialise
//! result. Workspace-owned targets fall through to the standard
//! rewrite chain so the canonical reuse path keeps ownership.

use verter_type_expr::{LiteralValue, TypeExpr};

use super::core::{peel_paren, DeclLookup, PolicyCtx};

/// Selective `Pick` / symbolic `Omit` handling when the target
/// declaration's source resolves to a package-backed canonical id.
/// Returns:
/// - `Some(...)` when the target is package-backed AND the helper
///   produced a definitive result (Pick: object with picked members;
///   Omit: symbolic Ref preserved).
/// - `None` when the target is workspace-owned, the keys can't be
///   extracted, or the target body doesn't fit the helper's contract;
///   the caller falls through to the standard rewrite chain so the
///   canonical reuse path keeps ownership.
pub(super) fn rewrite_pick_or_omit_for_package_backed(
    utility_name: &str,
    type_arguments: &[TypeExpr],
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<TypeExpr> {
    use crate::meta_resolve::materialize::utility_types::{
        selective_pick_expansion_for_package_backed, symbolic_omit_for_package_backed,
    };
    let target_arg = peel_paren(&type_arguments[0]);
    let keys_arg = type_arguments[1].clone();
    let TypeExpr::Ref {
        name: target_name,
        type_arguments: target_type_args,
    } = target_arg
    else {
        return None;
    };
    if !target_type_args.is_empty() {
        // Generic-parameterised target — let the standard chain
        // handle it (the helper works on bare alias bodies).
        return None;
    }
    let DeclLookup {
        canonical_source,
        body,
    } = ctx.locate_declaration(target_name.as_ref())?;
    if !canonical_source.contains("/node_modules/") {
        // Workspace-owned target: defer to the canonical reuse path.
        return None;
    }
    if utility_name == "Omit" {
        // Symbolic preservation — return the original
        // `Omit<target, keys>` shape unchanged. No member of the
        // target is enumerated.
        return Some(symbolic_omit_for_package_backed(
            type_arguments[0].clone(),
            keys_arg,
        ));
    }
    // Pick: extract the literal key set and materialise selectively.
    let keys = extract_pick_omit_string_literal_keys(&keys_arg)?;
    if keys.is_empty() {
        return None;
    }
    selective_pick_expansion_for_package_backed(&body, &keys, target_name.as_ref())
}

/// Extract a flat `Vec<String>` of string-literal keys from a
/// `Pick<T, K>` / `Omit<T, K>` second type argument. The shape is
/// either a single `Literal::String` or a `Union` of literal strings;
/// any other shape returns `None` so the caller falls through.
pub(super) fn extract_pick_omit_string_literal_keys(expr: &TypeExpr) -> Option<Vec<String>> {
    match peel_paren(expr) {
        TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
        TypeExpr::Union(arms) => {
            let mut out = Vec::with_capacity(arms.len());
            for arm in arms.iter() {
                match peel_paren(arm) {
                    TypeExpr::Literal(LiteralValue::String(value)) => out.push(value.clone()),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}
