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

use verter_type_expr::{FunctionExpr, ObjectMember, TypeExpr, TypeParam};

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
/// # Depth-safe
///
/// The walk is ITERATIVE over an explicit heap work-stack of `&TypeExpr`
/// children — NOT call-stack recursion — mirroring the crate's existing
/// depth-safe `Drop`/`Hash` walkers
/// (`verter_type_expr::recursive_traversal`). Real TypeScript lowers into
/// `TypeExpr` chains thousands of levels deep (`Array<Array<…>>`, long
/// `extends ? :` chains, deeply-parenthesised unions); a naive recursive
/// predicate would overflow the thread stack BEFORE the offending surface is
/// ever interned. Every directly-owned `TypeExpr` child of a popped node is
/// pushed back onto the stack and the loop drains them, so the native stack
/// stays flat regardless of tree depth.
///
/// # Exhaustive
///
/// [`push_type_expr_children`] matches EVERY `TypeExpr` variant with NO `_`
/// wildcard and pushes every nested `TypeExpr` child — including the children
/// the sibling `type_expr_contains_*` predicates treat as terminals
/// (`TypeOf`'s `ValueRef::type_args`, `TypeParameter`'s `constraint`/`default`,
/// and `RecursiveRef`'s `type_arguments` + `conditional_context` frames) AND
/// the `FunctionExpr.type_parameters` carried by every function-typed member
/// (the `Function`/`ConstructorType` variants and the `Method`/`CallSignature`/
/// `ConstructSignature` object members) — so no carrier buried under any
/// container, including a `<T extends Carrier>` constraint or a `<T = Carrier>`
/// default on a function signature's own type parameters, escapes detection.
/// Because there is no `_` wildcard, a future child-bearing `TypeExpr` (or
/// `ObjectMember`) variant fails to compile here rather than silently dropping
/// its subtree.
pub(crate) fn type_expr_contains_synthetic_slot_binding(expr: &TypeExpr) -> bool {
    let mut stack: Vec<&TypeExpr> = vec![expr];
    while let Some(node) = stack.pop() {
        // The carrier itself — the thing the invariant forbids. Found anywhere
        // in the tree, the walk short-circuits.
        if matches!(node, TypeExpr::SyntheticSlotBinding(_)) {
            return true;
        }
        push_type_expr_children(node, &mut stack);
    }
    false
}

/// Push every directly-owned nested `TypeExpr` child of `node` onto `stack`.
///
/// EXHAUSTIVE over every `TypeExpr` variant with NO `_` wildcard: a new
/// child-bearing variant fails to compile here rather than silently dropping
/// its subtree. The field coverage is cross-checked against the authoritative
/// field lists in `verter_type_expr::lib` and against the crate's depth-safe
/// `Drop`/`Hash` walkers (`recursive_traversal::drain_children` /
/// `hash_node`) — every field that can transitively hold a `TypeExpr` is
/// descended.
fn push_type_expr_children<'a>(node: &'a TypeExpr, stack: &mut Vec<&'a TypeExpr>) {
    match node {
        // The carrier is handled (and short-circuits) at the pop site; it owns
        // a `SyntheticCarrierKey` with no nested `TypeExpr` child.
        TypeExpr::SyntheticSlotBinding(_) => {}

        // Single-child wrappers.
        TypeExpr::Parenthesized(inner)
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner)
        | TypeExpr::Array { element: inner, .. } => stack.push(inner),

        // Flat child collections.
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => stack.extend(types.iter()),
        TypeExpr::Tuple { elements, .. } => {
            stack.extend(elements.iter().map(|element| &element.ty))
        }
        TypeExpr::TemplateLiteral { expressions, .. } => stack.extend(expressions.iter()),

        // Named references carry their applied type arguments.
        TypeExpr::Ref { type_arguments, .. } | TypeExpr::ImportType { type_arguments, .. } => {
            stack.extend(type_arguments.iter())
        }
        // A `RecursiveRef` carries its applied type arguments AND the
        // conditional-context frames captured at recursion detection; both can
        // hold buried carriers.
        TypeExpr::RecursiveRef {
            type_arguments,
            conditional_context,
            ..
        } => {
            stack.extend(type_arguments.iter());
            for frame in conditional_context.iter() {
                stack.push(&frame.check);
                stack.push(&frame.extends);
            }
        }

        // `typeof x.y<Args>` — the instantiation type arguments are children.
        TypeExpr::TypeOf(value_ref) => stack.extend(value_ref.type_args.iter()),
        // A type-parameter reference carries its constraint and default.
        TypeExpr::TypeParameter(type_param) => push_type_param_children(type_param, stack),

        // Object members: properties, methods, index signatures, and
        // call/construct signatures all carry nested types. A method / call /
        // construct signature carries a `FunctionExpr`, whose parameters,
        // return type, AND own type parameters are all descended.
        TypeExpr::Object(object) => {
            for member in object.properties.iter() {
                match member {
                    ObjectMember::Property(property) => stack.push(&property.ty),
                    ObjectMember::Method(method) => {
                        push_function_expr_children(&method.function, stack)
                    }
                    ObjectMember::IndexSignature(signature) => {
                        stack.push(&signature.key_type);
                        stack.push(&signature.value_type);
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        push_function_expr_children(function, stack)
                    }
                }
            }
        }
        // A function / constructor type's parameters, return type, AND own type
        // parameters.
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            push_function_expr_children(function, stack)
        }

        TypeExpr::IndexedAccess { object, index } => {
            stack.push(object);
            stack.push(index);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            stack.push(check);
            stack.push(extends);
            stack.push(true_type);
            stack.push(false_type);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            stack.push(source);
            stack.push(value);
            if let Some(name_type) = name_type.as_deref() {
                stack.push(name_type);
            }
        }

        // Genuine terminals — no nested `TypeExpr` child.
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. } => {}
    }
}

/// Push every nested `TypeExpr` child of a [`FunctionExpr`]: each parameter's
/// type, the return type, AND every own type parameter's `constraint` /
/// `default`.
///
/// `type_parameters` is the field the parameters-and-return-only predecessor
/// omitted, so a carrier under a function signature's own
/// `<T extends Carrier>` / `<T = Carrier>` escaped detection. The crate's
/// depth-safe drop walker (`recursive_traversal::drain_function_expr`)
/// descends the identical three field groups.
fn push_function_expr_children<'a>(function: &'a FunctionExpr, stack: &mut Vec<&'a TypeExpr>) {
    stack.extend(function.parameters.iter().map(|parameter| &parameter.ty));
    if let Some(return_type) = function.return_type.as_deref() {
        stack.push(return_type);
    }
    for type_param in function.type_parameters.iter() {
        push_type_param_children(type_param, stack);
    }
}

/// Push the `constraint` and `default` `TypeExpr` children of a [`TypeParam`].
fn push_type_param_children<'a>(type_param: &'a TypeParam, stack: &mut Vec<&'a TypeExpr>) {
    if let Some(constraint) = type_param.constraint.as_deref() {
        stack.push(constraint);
    }
    if let Some(default) = type_param.default.as_deref() {
        stack.push(default);
    }
}
