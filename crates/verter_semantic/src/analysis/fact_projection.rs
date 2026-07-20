//! Shared fact producers proving the field-to-fact mapping obligation via
//! EXHAUSTIVE destructuring.
//!
//! A fact producer destructures its source struct without `..`, so ADDING a
//! source field fails compilation until it is mapped to a fact field, an origin
//! locator, or a documented display-only carve-out. That compiler-level
//! obligation is what keeps the metadata-loss class closed — the structural
//! replacement for the removed name-keyed field-preservation scanner.
//!
//! [`value_type_annotation_fact`] is the SINGLE producer of the value
//! annotation fact's typeof-peel rule (the inventory builder mints through it —
//! the rule is never re-derived elsewhere). Producing a fact MAY read a
//! transient `TypeExpr`-shaped value; the produced fact carries none.

#![allow(dead_code)]

use std::sync::Arc;

use verter_type_expr::facts::{
    InferenceUnavailableReason, ResolvedLocalTypeFact, SemanticTypeSource, ValueAnnotationClass,
    ValueDeclIdentityPart, ValueTypeAnnotationFact,
};
use verter_type_expr::{TopLevelOwnerId, TypeExpr};

use crate::analysis::types::ResolvedLocalType;

/// Project a synthesized [`ResolvedLocalType`] into the closed
/// [`ResolvedLocalTypeFact`]. The source is destructured EXHAUSTIVELY: adding a
/// field to `ResolvedLocalType` fails compilation until it is mapped here. The
/// stored shape is already the closed shallow-by-default schema (a primitive
/// leaf or a shallow named-reference locator), produced where the analyzer
/// resolved the local type.
pub(crate) fn build_resolved_local_type_fact(src: &ResolvedLocalType) -> ResolvedLocalTypeFact {
    // EXHAUSTIVE destructure — every field named, none elided via `..`.
    let ResolvedLocalType {
        name,
        owner,
        // Display-only expanded-type TEXT — a carve-out, never a semantic fact.
        expanded: _display_only_expanded,
        shape,
        // The reference span is addressed by the enclosing macro payload locator
        // (the fact carries no top-level span field); recovered-via-locator,
        // never stored.
        span: _reference_span_recovered_via_locator,
    } = src;

    ResolvedLocalTypeFact {
        name: name.clone(),
        owner: *owner,
        shape: shape.clone(),
    }
}

/// Produce the [`ValueTypeAnnotationFact`] for a value declaration's authored
/// annotation.
///
/// `typeof_alias_target` is populated IFF the annotation is a SINGLE-HOP
/// `typeof x` value peel whose target is not the declaration itself:
///
/// - multi-hop `typeof x.y` → `None` (a member projection, not a value-alias
///   peel);
/// - self-referential `typeof own` → `None` (the self-reference break — the
///   peel target decision IS the termination guard, so a self-peel must never
///   produce a follow edge);
/// - single-hop non-self `typeof x` → `Some` (the stored graph-free peel
///   target replacing the `TypeExpr::TypeOf` walk).
///
/// The classification pairs with the target: [`ValueAnnotationClass::TypeOfAlias`]
/// exactly when a peel target is stored; any other present annotation is
/// [`ValueAnnotationClass::Direct`]; an absent annotation is
/// [`ValueAnnotationClass::Absent`]. Reading the transient `TypeExpr` here is
/// producer-legal; the produced fact carries none.
pub(crate) fn value_type_annotation_fact(
    annotation: Option<&TypeExpr>,
    own_decl_name: &str,
    declaring_canonical: &Arc<str>,
    owner: TopLevelOwnerId,
    annotation_source: Option<SemanticTypeSource>,
    inference_unavailable: Option<InferenceUnavailableReason>,
) -> ValueTypeAnnotationFact {
    if let Some(reason) = inference_unavailable {
        debug_assert!(annotation.is_none());
        debug_assert!(annotation_source.is_none());
        return ValueTypeAnnotationFact {
            typeof_alias_target: None,
            classification: ValueAnnotationClass::InferenceUnavailable(reason),
            annotation: None,
        };
    }
    let Some(annotation) = annotation else {
        debug_assert!(
            annotation_source.is_none(),
            "annotation/source pairing: an absent annotation carries no source"
        );
        return ValueTypeAnnotationFact {
            typeof_alias_target: None,
            classification: ValueAnnotationClass::Absent,
            annotation: None,
        };
    };

    let typeof_alias_target = match annotation {
        TypeExpr::TypeOf(value_ref)
            if value_ref.path.len() == 1 && value_ref.path[0] != own_decl_name =>
        {
            Some(ValueDeclIdentityPart {
                canonical_id: declaring_canonical.clone(),
                owner,
                symbol: Arc::from(value_ref.path[0].as_str()),
                member_path: Arc::from([]),
            })
        }
        _ => None,
    };
    let classification = if typeof_alias_target.is_some() {
        ValueAnnotationClass::TypeOfAlias
    } else {
        ValueAnnotationClass::Direct
    };
    ValueTypeAnnotationFact {
        typeof_alias_target,
        classification,
        annotation: annotation_source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_span::Span;
    use verter_type_expr::facts::{LeafTypeFact, ResolvedLocalShape};
    use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, SymbolBodyLocator};
    use verter_type_expr::PrimitiveName;

    fn leaf_shape(primitive: PrimitiveName) -> ResolvedLocalShape {
        ResolvedLocalShape::Leaf(LeafTypeFact::Primitive(primitive))
    }

    fn ref_shape(name: &str) -> ResolvedLocalShape {
        ResolvedLocalShape::Ref(SymbolBodyLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/ws/a.ts"),
                owner: TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from(name),
                space: LocatorSymbolSpace::Type,
            },
        })
    }

    fn local_type(name: &str, shape: ResolvedLocalShape) -> ResolvedLocalType {
        ResolvedLocalType {
            name: name.to_string(),
            owner: TopLevelOwnerId::ordinary_file(),
            expanded: "<expanded text>".to_string(),
            shape,
            span: Span::default(),
        }
    }

    #[test]
    fn primitive_shape_projects_to_a_leaf_fact() {
        let fact =
            build_resolved_local_type_fact(&local_type("N", leaf_shape(PrimitiveName::Number)));
        assert_eq!(fact.name, "N");
        assert_eq!(
            fact.shape,
            ResolvedLocalShape::Leaf(LeafTypeFact::Primitive(PrimitiveName::Number))
        );
    }

    #[test]
    fn reference_shapes_stay_shallow_refs() {
        let fact = build_resolved_local_type_fact(&local_type("Props", ref_shape("Props")));
        assert!(matches!(fact.shape, ResolvedLocalShape::Ref(_)));
    }

    #[test]
    fn shape_distinction_is_preserved_by_the_producer() {
        // Discriminating: a primitive shape and a reference shape produce
        // DISTINCT facts — the producer does not collapse them.
        let primitive =
            build_resolved_local_type_fact(&local_type("T", leaf_shape(PrimitiveName::String)));
        let reference = build_resolved_local_type_fact(&local_type("T", ref_shape("T")));
        assert_ne!(primitive, reference);
    }

    fn typeof_annotation(path: &[&str]) -> TypeExpr {
        TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: path.iter().map(|s| s.to_string()).collect(),
            type_args: Vec::new(),
        })
    }

    #[test]
    fn single_hop_non_self_typeof_stores_the_peel_target() {
        let canonical: Arc<str> = Arc::from("/ws/a.ts");
        let fact = value_type_annotation_fact(
            Some(&typeof_annotation(&["source"])),
            "alias",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
            None,
        );
        assert_eq!(fact.classification, ValueAnnotationClass::TypeOfAlias);
        let target = fact.typeof_alias_target.expect("single-hop peel target");
        assert_eq!(target.canonical_id.as_ref(), "/ws/a.ts");
        assert_eq!(target.symbol.as_ref(), "source");
        assert!(
            target.member_path.is_empty(),
            "a bare value peel has no member path"
        );
    }

    #[test]
    fn multi_hop_typeof_stores_no_peel_target() {
        let canonical: Arc<str> = Arc::from("/ws/a.ts");
        let fact = value_type_annotation_fact(
            Some(&typeof_annotation(&["obj", "member"])),
            "alias",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
            None,
        );
        assert_eq!(
            fact.typeof_alias_target, None,
            "typeof obj.member is a member projection, not a value-alias peel"
        );
        assert_eq!(fact.classification, ValueAnnotationClass::Direct);
    }

    #[test]
    fn self_referential_typeof_stores_no_peel_target() {
        // The self-reference break: `const own: typeof own` must not produce a
        // follow edge — the Some/None decision IS the termination guard.
        let canonical: Arc<str> = Arc::from("/ws/a.ts");
        let fact = value_type_annotation_fact(
            Some(&typeof_annotation(&["own"])),
            "own",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
            None,
        );
        assert_eq!(fact.typeof_alias_target, None, "self-peel must break");
        assert_eq!(fact.classification, ValueAnnotationClass::Direct);
    }

    #[test]
    fn absent_and_direct_annotations_classify_without_a_target() {
        let canonical: Arc<str> = Arc::from("/ws/a.ts");
        let absent = value_type_annotation_fact(
            None,
            "x",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
            None,
        );
        assert_eq!(absent.classification, ValueAnnotationClass::Absent);
        assert_eq!(absent.typeof_alias_target, None);
        assert_eq!(absent.annotation, None);

        let direct = value_type_annotation_fact(
            Some(&TypeExpr::Primitive(PrimitiveName::String)),
            "x",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            Some(SemanticTypeSource::Closed(
                verter_type_expr::facts::ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                    PrimitiveName::String,
                )),
            )),
            None,
        );
        assert_eq!(direct.classification, ValueAnnotationClass::Direct);
        assert_eq!(direct.typeof_alias_target, None);
        assert!(direct.annotation.is_some(), "direct source is carried");
    }
}
