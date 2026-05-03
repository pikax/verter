//! Slot-binding indexed-access symbolic preservation (Issue #1, partial).
//!
//! Force the slot binding's `type_expr` back to the symbolic
//! `IndexedAccess` shape encoded in `raw_type` when the indexed access
//! transits through an imported declaration. The eager evaluator may
//! have widened the access through an open `[k: string]: any` index
//! signature; the navigable member-path contract is the better public
//! surface.

use verter_semantic::analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

use super::core::{peel_paren, DeclLookup, PolicyCtx};

/// Whether the slot binding's `raw_type` describes an indexed access that
/// transits through an imported declaration. When true, the caller restores
/// the symbolic form from `raw_type` and skips the expansion walk.
pub(super) fn slot_binding_should_preserve_symbolic_raw_type(
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
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(trimmed);
    raw_indexed_access_root_is_imported(&parsed, ctx)
}

/// Parse the slot binding's `raw_type` annotation back to a `TypeExpr`.
/// Returns `None` for empty/missing raw types or when the parsed shape
/// is not an `IndexedAccess` (only IndexedAccess is restored by the
/// slot-binding guard).
pub(super) fn parse_indexed_access_from_raw(raw_type: Option<&str>) -> Option<TypeExpr> {
    let raw = raw_type?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(trimmed);
    if matches!(peel_paren(&parsed), TypeExpr::IndexedAccess { .. }) {
        Some(parsed)
    } else {
        None
    }
}

/// Returns true when `expr` is an `IndexedAccess` whose deref chain
/// transits through a Ref to an imported declaration. The "indexed
/// root" is the chain starting from the indexed access's `object` and
/// the property body that the access selects from the root's
/// declaration body.
fn raw_indexed_access_root_is_imported(expr: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    let TypeExpr::IndexedAccess { object, index } = peel_paren(expr) else {
        return false;
    };
    // Index must be a string literal — that is the member-path the
    // policy can statically inspect inside the root's declaration body.
    let TypeExpr::Literal(LiteralValue::String(member)) = peel_paren(index) else {
        return false;
    };
    // Peel through `Object & { … }` and `Object` shapes when the user
    // wrote the indexed access on a literal object (covered by
    // `reduce_indexed_access_over_object_surface`); for the slot
    // binding case we expect a Ref to a declaration.
    let TypeExpr::Ref { name, .. } = peel_paren(object) else {
        return false;
    };
    let Some(DeclLookup {
        canonical_source,
        body,
    }) = ctx.locate_declaration(name.as_ref())
    else {
        return false;
    };
    // The root's declaration body must be an Object whose `member`
    // property type contains an imported reference (or itself resolves
    // to an imported declaration).
    let property_type = match peel_paren(&body) {
        TypeExpr::Object(obj) => obj.properties.iter().find_map(|m| match m {
            ObjectMember::Property(p) if p.name == *member => Some(p.ty.clone()),
            _ => None,
        }),
        _ => None,
    };
    let Some(property_type) = property_type else {
        return false;
    };
    let _ = canonical_source; // root's own location is not the trigger
    type_expr_contains_imported_ref(&property_type, ctx)
}

/// Walks `expr` and returns true on the first `Ref` whose declaration
/// resolves to an imported (non-owner) declaration. Refs whose
/// declarations cannot be located are ignored — they cannot be proven
/// imported.
fn type_expr_contains_imported_ref(expr: &TypeExpr, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => type_expr_contains_imported_ref(inner, ctx),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(DeclLookup {
                canonical_source, ..
            }) = ctx.locate_declaration(name.as_ref())
            {
                if canonical_source != ctx.owner_canonical {
                    return true;
                }
            }
            type_arguments
                .iter()
                .any(|arg| type_expr_contains_imported_ref(arg, ctx))
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(|ty| type_expr_contains_imported_ref(ty, ctx)),
        TypeExpr::Array { element, .. } => type_expr_contains_imported_ref(element, ctx),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_contains_imported_ref(&element.ty, ctx)),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_contains_imported_ref(object, ctx)
                || type_expr_contains_imported_ref(index, ctx)
        }
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => type_expr_contains_imported_ref(&prop.ty, ctx),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|parameter| type_expr_contains_imported_ref(&parameter.ty, ctx))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|rt| type_expr_contains_imported_ref(rt, ctx))
            }
            ObjectMember::IndexSignature(sig) => {
                type_expr_contains_imported_ref(&sig.key_type, ctx)
                    || type_expr_contains_imported_ref(&sig.value_type, ctx)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| type_expr_contains_imported_ref(&parameter.ty, ctx))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|rt| type_expr_contains_imported_ref(rt, ctx))
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| type_expr_contains_imported_ref(&parameter.ty, ctx))
                || function
                    .return_type
                    .as_deref()
                    .is_some_and(|rt| type_expr_contains_imported_ref(rt, ctx))
        }
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_imported_ref(inner, ctx)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_contains_imported_ref(check, ctx)
                || type_expr_contains_imported_ref(extends, ctx)
                || type_expr_contains_imported_ref(true_type, ctx)
                || type_expr_contains_imported_ref(false_type, ctx)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_contains_imported_ref(source, ctx)
                || type_expr_contains_imported_ref(value, ctx)
                || name_type
                    .as_deref()
                    .is_some_and(|nt| type_expr_contains_imported_ref(nt, ctx))
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|e| type_expr_contains_imported_ref(e, ctx)),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. } => false,
    }
}

/// Reduce an indexed access on an already-concrete object surface.
/// `{ avatar: T }['avatar']` reduces directly to `T` without consulting
/// the resolver — the object literal is a complete shape so the lookup
/// is purely structural. Used by the slot-binding policy to short-circuit
/// when the materializer's expansion produced a known-shape `Object`
/// where the index-access target is already in the property list.
#[allow(dead_code)]
pub(super) fn reduce_indexed_access_over_object_surface(
    object: &TypeExpr,
    index: &TypeExpr,
) -> Option<TypeExpr> {
    let TypeExpr::Object(obj) = peel_paren(object) else {
        return None;
    };
    let TypeExpr::Literal(LiteralValue::String(member)) = peel_paren(index) else {
        return None;
    };
    obj.properties.iter().find_map(|m| match m {
        ObjectMember::Property(p) if p.name == *member => Some(p.ty.clone()),
        _ => None,
    })
}
