//! Reducible-operator structural predicate.
//!
//! This module hosts the structural predicate that decides whether the
//! bounded fixed-point reducer needs to run for a given `TypeExpr`. The
//! published-surface field-type driver (`reduce_published_field_types`) and the
//! per-field reducer (`reduce_field_type_expr_with_mode`) live in the terminal
//! `output_sink` sink module (the only module that touches the reverse
//! materialization boundary); this predicate is pure typed-IR inspection with
//! no boundary access, so it stays a free-standing `pub(crate)` helper both the
//! sink reducer and the per-kind projectors consume.
//!
//! Historical note: the retired `field_reduce.rs` module hosted the
//! field-type reducer plus a projector-side name-predicate carrier
//! check (`is_builtin_utility_instantiation`,
//! `generic_instantiation_body_is_object`). The demand-driven reducer retires
//! the projector-side carrier check by routing carrier-stop through
//! the dispatch demand context — only the type-shape predicate remains here.

use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
use verter_type_expr::TypeExpr;

/// Does `expr` contain any operator-shape node that the bounded
/// fixed-point reducer should resolve?
///
/// Returns `true` when the expression carries an `IndexedAccess`,
/// `KeyOf`, `TypeOf`, `Conditional`, `Mapped`, or `Infer` anywhere
/// in its tree, or an applied builtin-utility reference
/// (`Partial<EditorOptions>`, `Pick<Foo, 'bar'>`, …) — an explicit
/// utility in the published type IS explicit consumer demand, and
/// carrier-preserving lowering hands it to the reducer un-executed.
/// The reducer's `Navigate` reduction then decides
/// closed→materialise / open→carrier through the shared L1
/// predicates. Used by `reduce_field_type_expr_with_mode` to decide
/// whether to skip the reducer entirely.
pub(crate) fn type_expr_contains_reducible_operator(expr: &TypeExpr) -> bool {
    use verter_type_expr::ObjectMember;

    match expr {
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::Infer { .. }
        // An import-type is a cross-file resolution carrier the reducer
        // must resolve — grouped with the operator-shape arms.
        | TypeExpr::ImportType { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } if !type_arguments.is_empty() && BuiltinUtility::from_name(name.as_ref()).is_some() => {
            true
        }
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            type_expr_contains_reducible_operator(inner)
        }
        TypeExpr::Array { element, .. } => type_expr_contains_reducible_operator(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|el| type_expr_contains_reducible_operator(&el.ty)),
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().any(type_expr_contains_reducible_operator)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
            ObjectMember::Property(p) => type_expr_contains_reducible_operator(&p.ty),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_reducible_operator(&param.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
            ObjectMember::IndexSignature(sig) => {
                type_expr_contains_reducible_operator(&sig.key_type)
                    || type_expr_contains_reducible_operator(&sig.value_type)
            }
            ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                f.parameters
                    .iter()
                    .any(|p| type_expr_contains_reducible_operator(&p.ty))
                    || f.return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_reducible_operator)
            }
        }),
        // A constructor type's signature is searched identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(f) | TypeExpr::ConstructorType(f) => {
            f.parameters
                .iter()
                .any(|p| type_expr_contains_reducible_operator(&p.ty))
                || f.return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_reducible_operator)
        }
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_reducible_operator),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers carry no reducible operators — they are
        // intrinsic terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => false,
    }
}
