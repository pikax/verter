//! Synthetic-carrier recogniser for the `VueMacroElements` construction-site
//! invariant.
//!
//! Extracted to a sibling so the hot-path memo logic in the crate root stays
//! under the Tier-2 module-size budget (the same reason `arena` / `interner` /
//! `test_gates` live beside it).
//!
//! [`type_expr_contains_synthetic_slot_binding`] backs the checked invariant in
//! [`SemanticGraphStore::insert_resolved_named_type`](super::SemanticGraphStore::insert_resolved_named_type):
//! the parser-built macro-elements surface reaching the `VueMacroElements` slot
//! must never carry a `TypeExpr::SyntheticSlotBinding` ordinal in any member
//! `type_expr`, because the footprint encoder Debug-folds that arm and a carrier
//! there would leak a store/generation-relative `SemanticNodeId` arena ordinal
//! (`SyntheticCarrierKey.value_node`) into the otherwise content-only
//! fingerprint.

use verter_type_expr::{ObjectMember, TypeExpr};

/// Checked invariant for the `VueMacroElements` construction site: the
/// parser-built macro-elements surface must never carry a
/// `TypeExpr::SyntheticSlotBinding` ordinal in any member type.
///
/// The caller passes every member `type_expr` (each an `Option<TypeExpr>` — the
/// props' and the named call signatures' lowered types) and this hard-`assert!`s
/// (NOT a release-erased `debug_assert!`, so the invariant holds in release too)
/// that none contains a carrier — because the footprint encoder Debug-folds the
/// `VueMacroElements` arm and a carrier there would leak a
/// store/generation-relative `SemanticNodeId` arena ordinal
/// (`SyntheticCarrierKey.value_node`) into the otherwise content-only
/// fingerprint and break cross-host byte identity. The producer surface is
/// provably carrier-free today, so this is a fixed-shape, no-allocation walk
/// that never fires on the real tree.
///
/// Taking an iterator of `&Option<TypeExpr>` keeps this helper
/// container-agnostic — it names only `TypeExpr`, never the macro-elements
/// surface type, and is NOT a resolution engine / projector / surface walker.
/// Called immediately before
/// [`SemanticGraphStore::intern_node`](super::SemanticGraphStore::intern_node)
/// in [`insert_resolved_named_type`](super::SemanticGraphStore::insert_resolved_named_type)
/// so the enforcement is synchronous at the single construction site.
pub(crate) fn assert_no_synthetic_carrier<'a>(
    member_type_exprs: impl Iterator<Item = &'a Option<TypeExpr>>,
) {
    let carrier_member = member_type_exprs
        .filter_map(Option::as_ref)
        .any(type_expr_contains_synthetic_slot_binding);
    assert!(
        !carrier_member,
        "the parser-built macro-elements surface reaching `VueMacroElements` \
         must never carry a `TypeExpr::SyntheticSlotBinding` ordinal — the \
         footprint encoder Debug-hashes it; a carrier here would leak a \
         `SemanticNodeId` arena ordinal into the content fingerprint. This is a \
         parser-built surface and structurally must not contain a \
         session-minted slot-binding carrier.",
    );
}

/// Returns `true` iff `expr` IS, or transitively CONTAINS, a
/// [`TypeExpr::SyntheticSlotBinding`] carrier.
///
/// The match is EXHAUSTIVE over every `TypeExpr` variant and descends every
/// nested `TypeExpr` child — including the children the sibling
/// `type_expr_contains_*` predicates treat as terminals (`TypeOf`'s
/// `ValueRef::type_args`, `TypeParameter`'s `constraint`/`default`, and
/// `RecursiveRef`'s `type_arguments` + `conditional_context` frames) — so no
/// carrier buried under any container escapes detection. There is no `_`
/// wildcard: adding a future child-bearing variant fails to compile here rather
/// than silently dropping its subtree.
pub(crate) fn type_expr_contains_synthetic_slot_binding(expr: &TypeExpr) -> bool {
    match expr {
        // The carrier itself — the thing the invariant forbids.
        TypeExpr::SyntheticSlotBinding(_) => true,

        // Single-child wrappers.
        TypeExpr::Parenthesized(inner)
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner)
        | TypeExpr::Array { element: inner, .. } => {
            type_expr_contains_synthetic_slot_binding(inner)
        }

        // Flat child collections.
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(type_expr_contains_synthetic_slot_binding)
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_contains_synthetic_slot_binding(&element.ty)),
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_synthetic_slot_binding),

        // Named references carry their applied type arguments.
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            type_arguments
                .iter()
                .any(type_expr_contains_synthetic_slot_binding)
        }
        // A `RecursiveRef` carries its applied type arguments AND the
        // conditional-context frames captured at recursion detection; both can
        // hold buried carriers.
        TypeExpr::RecursiveRef {
            type_arguments,
            conditional_context,
            ..
        } => {
            type_arguments
                .iter()
                .any(type_expr_contains_synthetic_slot_binding)
                || conditional_context.iter().any(|frame| {
                    type_expr_contains_synthetic_slot_binding(&frame.check)
                        || type_expr_contains_synthetic_slot_binding(&frame.extends)
                })
        }

        // `typeof x.y<Args>` — the instantiation type arguments are children.
        TypeExpr::TypeOf(value_ref) => value_ref
            .type_args
            .iter()
            .any(type_expr_contains_synthetic_slot_binding),
        // A type-parameter reference carries its constraint and default.
        TypeExpr::TypeParameter(type_param) => {
            type_param
                .constraint
                .as_deref()
                .is_some_and(type_expr_contains_synthetic_slot_binding)
                || type_param
                    .default
                    .as_deref()
                    .is_some_and(type_expr_contains_synthetic_slot_binding)
        }

        // Object members: properties, methods, index signatures, and
        // call/construct signatures all carry nested types.
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => {
                type_expr_contains_synthetic_slot_binding(&property.ty)
            }
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|parameter| type_expr_contains_synthetic_slot_binding(&parameter.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_synthetic_slot_binding)
            }
            ObjectMember::IndexSignature(signature) => {
                type_expr_contains_synthetic_slot_binding(&signature.key_type)
                    || type_expr_contains_synthetic_slot_binding(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| type_expr_contains_synthetic_slot_binding(&parameter.ty))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_synthetic_slot_binding)
            }
        }),
        // A function / constructor type's parameters and return type.
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function
                .parameters
                .iter()
                .any(|parameter| type_expr_contains_synthetic_slot_binding(&parameter.ty))
                || function
                    .return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_synthetic_slot_binding)
        }

        TypeExpr::IndexedAccess { object, index } => {
            type_expr_contains_synthetic_slot_binding(object)
                || type_expr_contains_synthetic_slot_binding(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_contains_synthetic_slot_binding(check)
                || type_expr_contains_synthetic_slot_binding(extends)
                || type_expr_contains_synthetic_slot_binding(true_type)
                || type_expr_contains_synthetic_slot_binding(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_contains_synthetic_slot_binding(source)
                || type_expr_contains_synthetic_slot_binding(value)
                || name_type
                    .as_deref()
                    .is_some_and(type_expr_contains_synthetic_slot_binding)
        }

        // Genuine terminals — no nested `TypeExpr` child.
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. } => false,
    }
}
