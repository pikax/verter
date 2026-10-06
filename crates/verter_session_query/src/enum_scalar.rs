//! The canonical conversion of an enum member scalar to its literal type.

use verter_type_expr::facts::{EnumPrimitiveDomain, EnumScalar};
use verter_type_expr::{PrimitiveName, TypeExpr};

/// The scalar → projected-`TypeExpr` mapping for a stored enum member fact —
/// the session-side reader of the closed [`EnumScalar`] vocabulary (a folded
/// numeric scalar stores the CANONICAL `f64` display string, so the parse-back
/// recovers the exact bits; a deferred member's domain maps to its degraded
/// sound arm). Mirrors the `verter_semantic` fingerprint producer's
/// `scalar_to_type_expr` mapping — the shared closed grammar, not a resolver.
pub fn enum_scalar_type_expr(scalar: &EnumScalar) -> TypeExpr {
    match scalar {
        EnumScalar::String(value) => TypeExpr::string_literal(value.as_str()),
        EnumScalar::Number(value) => TypeExpr::number_literal(
            value
                .parse::<f64>()
                .expect("EnumScalar::Number stores the canonical f64 display string"),
        ),
        EnumScalar::Primitive(domain) => match domain {
            EnumPrimitiveDomain::Number => TypeExpr::Primitive(PrimitiveName::Number),
            EnumPrimitiveDomain::String => TypeExpr::Primitive(PrimitiveName::String),
            EnumPrimitiveDomain::NumberOrString => TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::Number),
                TypeExpr::Primitive(PrimitiveName::String),
            ]),
            EnumPrimitiveDomain::Unknown => TypeExpr::Primitive(PrimitiveName::Unknown),
        },
    }
}
