//! Demo/witness fact producer proving the field-to-fact mapping obligation via
//! EXHAUSTIVE destructuring.
//!
//! A fact producer destructures its source struct without `..`, so ADDING a
//! source field fails compilation until it is mapped to a fact field, an origin
//! locator, or a documented display-only carve-out. That compiler-level
//! obligation is what keeps the metadata-loss class closed — the structural
//! replacement for the removed name-keyed field-preservation scanner.
//!
//! This is the synthesized [`ResolvedLocalType`] path — the family this layer can
//! genuinely produce without analyzer origin-path emission (which is a later
//! producer). Producing a fact MAY read an internal `TypeExpr`-shaped value; the
//! produced fact carries none.

#![allow(dead_code)]

use std::sync::Arc;

use verter_type_expr::facts::{LeafTypeFact, ResolvedLocalShape, ResolvedLocalTypeFact};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, SymbolBodyLocator};
use verter_type_expr::TypeExpr;

use crate::analysis::types::ResolvedLocalType;

/// Project a synthesized [`ResolvedLocalType`] into the closed
/// [`ResolvedLocalTypeFact`]. The source is destructured EXHAUSTIVELY: adding a
/// field to `ResolvedLocalType` fails compilation until it is mapped here. Real
/// object-member origin-path emission is handled by a later producer; this demo
/// produces a shallow-by-default shape.
pub(crate) fn build_resolved_local_type_fact(
    src: &ResolvedLocalType,
    producing_canonical: Arc<str>,
) -> ResolvedLocalTypeFact {
    // EXHAUSTIVE destructure — every field named, none elided via `..`.
    let ResolvedLocalType {
        name,
        // Display-only expanded-type TEXT — a carve-out, never a semantic fact.
        expanded: _display_only_expanded,
        type_expr,
        // The reference span is addressed by the enclosing macro payload locator
        // (the fact carries no top-level span field); recovered-via-locator,
        // never stored.
        span: _reference_span_recovered_via_locator,
    } = src;

    let anchor = AuthoredAnchor {
        canonical_id: producing_canonical,
        symbol: Arc::from(name.as_str()),
        space: LocatorSymbolSpace::Type,
    };

    // Shallow-by-default classification: a primitive folds to a leaf fact; every
    // other body stays a shallow named reference resolved on demand.
    let shape = match type_expr {
        Some(TypeExpr::Primitive(primitive)) => {
            ResolvedLocalShape::Leaf(LeafTypeFact::Primitive(*primitive))
        }
        _ => ResolvedLocalShape::Ref(SymbolBodyLocator { anchor }),
    };

    ResolvedLocalTypeFact {
        name: name.clone(),
        shape,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_span::Span;
    use verter_type_expr::PrimitiveName;

    fn local_type(name: &str, type_expr: Option<TypeExpr>) -> ResolvedLocalType {
        ResolvedLocalType {
            name: name.to_string(),
            expanded: "<expanded text>".to_string(),
            type_expr,
            span: Span::default(),
        }
    }

    #[test]
    fn primitive_body_folds_to_a_leaf_fact() {
        let fact = build_resolved_local_type_fact(
            &local_type("N", Some(TypeExpr::Primitive(PrimitiveName::Number))),
            Arc::from("/ws/a.ts"),
        );
        assert_eq!(fact.name, "N");
        assert_eq!(
            fact.shape,
            ResolvedLocalShape::Leaf(LeafTypeFact::Primitive(PrimitiveName::Number))
        );
    }

    #[test]
    fn non_primitive_and_absent_bodies_stay_shallow_refs() {
        let none_fact =
            build_resolved_local_type_fact(&local_type("Props", None), Arc::from("/ws/a.ts"));
        assert!(matches!(none_fact.shape, ResolvedLocalShape::Ref(_)));
    }

    #[test]
    fn shape_distinction_is_preserved_by_the_producer() {
        // Discriminating: a primitive body and a non-primitive body produce
        // DISTINCT facts — the producer does not collapse them.
        let primitive = build_resolved_local_type_fact(
            &local_type("T", Some(TypeExpr::Primitive(PrimitiveName::String))),
            Arc::from("/ws/a.ts"),
        );
        let reference =
            build_resolved_local_type_fact(&local_type("T", None), Arc::from("/ws/a.ts"));
        assert_ne!(primitive, reference);
    }
}
