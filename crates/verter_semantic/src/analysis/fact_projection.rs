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
    AuthoredReferenceArgLocator, AuthoredReferenceHeadFact, InferenceUnavailableReason,
    ResolvedLocalTypeFact, SemanticTypeSource, ValueAnnotationClass, ValueDeclIdentityPart,
    ValueTypeAnnotationFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, TypeArgLocator, TypeBodyPathStep,
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn value_type_annotation_fact(
    annotation: Option<&TypeExpr>,
    is_unique_symbol: bool,
    own_decl_name: &str,
    declaring_canonical: &Arc<str>,
    owner: TopLevelOwnerId,
    annotation_source: Option<SemanticTypeSource>,
    expression_source: Option<verter_type_expr::facts::SemanticExpressionSource>,
    inference_unavailable: Option<InferenceUnavailableReason>,
) -> ValueTypeAnnotationFact {
    if let Some(reason) = inference_unavailable {
        verter_debug_assert!(annotation.is_none());
        verter_debug_assert!(annotation_source.is_none());
        verter_debug_assert!(expression_source.is_none());
        return ValueTypeAnnotationFact {
            is_unique_symbol: false,
            typeof_alias_target: None,
            classification: ValueAnnotationClass::InferenceUnavailable(reason),
            annotation: None,
            reference_head: AuthoredReferenceHeadFact::Unavailable,
            expression_source: None,
        };
    }
    let Some(annotation) = annotation else {
        if expression_source.is_some() {
            verter_debug_assert!(annotation_source.is_none());
            return ValueTypeAnnotationFact {
                is_unique_symbol: false,
                typeof_alias_target: None,
                classification: ValueAnnotationClass::Direct,
                annotation: None,
                reference_head: AuthoredReferenceHeadFact::NotReference,
                expression_source,
            };
        }
        verter_debug_assert!(
            annotation_source.is_none(),
            "annotation/source pairing: an absent annotation carries no source"
        );
        return ValueTypeAnnotationFact {
            is_unique_symbol: false,
            typeof_alias_target: None,
            classification: ValueAnnotationClass::Absent,
            annotation: None,
            reference_head: AuthoredReferenceHeadFact::NotReference,
            expression_source: None,
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
        is_unique_symbol,
        typeof_alias_target,
        classification,
        annotation: annotation_source,
        reference_head: authored_reference_head_fact_with(annotation, |arg_index| {
            Some(AuthoredReferenceArgLocator::Value(TypeArgLocator {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::clone(declaring_canonical),
                    owner,
                    symbol: Arc::from(own_decl_name),
                    space: LocatorSymbolSpace::Value,
                },
                path: Arc::from([]),
                arg_index: u32::try_from(arg_index).ok()?,
            }))
        }),
        expression_source: None,
    }
}

/// Mint the closed authored reference head of a VALUE-SPACE function
/// signature's AUTHORED return annotation, while the signature producer already
/// holds the transient lowered return `TypeExpr`.
///
/// `anchor` is the owning value declaration's authored anchor and `first_step`
/// roots the argument locators at the signature's authored position — the same
/// `(anchor, first_step)` pair the signature's parameter / return body locators
/// use — so the head's arguments address
/// `[first_step, FunctionReturn]` at `arg_index`.
///
/// The CALLER owns the authorship gate: this entry must be reached only for a
/// signature whose return annotation was actually authored. Reading the
/// transient `TypeExpr` here is producer-legal; the produced fact carries none.
pub(crate) fn signature_return_reference_head_fact(
    annotation: &TypeExpr,
    anchor: &AuthoredAnchor,
    first_step: TypeBodyPathStep,
) -> AuthoredReferenceHeadFact {
    authored_reference_head_fact_with(annotation, |arg_index| {
        Some(AuthoredReferenceArgLocator::Value(TypeArgLocator {
            anchor: anchor.clone(),
            path: Arc::from([first_step, TypeBodyPathStep::FunctionReturn]),
            arg_index: u32::try_from(arg_index).ok()?,
        }))
    })
}

/// Mint the closed authored reference head of ONE prepared type-decl MEMBER's
/// AUTHORED annotation, while the member indexer already holds the transient
/// declaration body.
///
/// `anchor` is the owning declaration's authored anchor and `member_value_path`
/// is the member's own body path (`[..prefix, Member { ordinal }, MemberValue]`)
/// — the SAME `(anchor, path)` pair the member's `ty` body locator carries — so
/// the head's arguments address the authored annotation itself rather than the
/// declaration body root.
///
/// The CALLER owns the authorship gate: this entry must be reached only for a
/// property member whose `spans.type_annotation` is `Some` — a property's `ty`
/// is NOT always authored (an initializer-only class field carries an INFERRED
/// type), and an unauthored member publishes
/// [`AuthoredReferenceHeadFact::Unavailable`] without reaching here, exactly as
/// a method member (which has no member type annotation at all) does. Reading
/// the transient `TypeExpr` here is producer-legal; the produced fact carries
/// none.
pub(crate) fn member_annotation_reference_head_fact(
    annotation: &TypeExpr,
    anchor: &AuthoredAnchor,
    member_value_path: &[TypeBodyPathStep],
) -> AuthoredReferenceHeadFact {
    authored_reference_head_fact_with(annotation, |arg_index| {
        Some(AuthoredReferenceArgLocator::Value(TypeArgLocator {
            anchor: anchor.clone(),
            path: Arc::from(member_value_path.to_vec()),
            arg_index: u32::try_from(arg_index).ok()?,
        }))
    })
}

/// Mint the closed authored reference head for a macro field while the macro
/// hot-mirror producer already holds the transient payload body.
#[doc(hidden)]
pub fn macro_payload_reference_head_fact(
    annotation: &TypeExpr,
    payload: &MacroPayloadLocator,
) -> AuthoredReferenceHeadFact {
    authored_reference_head_fact_with(annotation, |arg_index| {
        Some(AuthoredReferenceArgLocator::MacroPayload {
            payload: payload.clone(),
            arg_index: u32::try_from(arg_index).ok()?,
        })
    })
}

fn authored_reference_head_fact_with(
    annotation: &TypeExpr,
    arg_locator: impl Fn(usize) -> Option<AuthoredReferenceArgLocator> + Copy,
) -> AuthoredReferenceHeadFact {
    let annotation = match annotation {
        TypeExpr::Parenthesized(inner) => {
            return authored_reference_head_fact_with(inner, arg_locator);
        }
        other => other,
    };
    let args = |len: usize| -> Option<Arc<[AuthoredReferenceArgLocator]>> {
        (0..len)
            .map(arg_locator)
            .collect::<Option<Vec<_>>>()
            .map(Arc::from)
    };
    match annotation {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let Some(args) = args(type_arguments.len()) else {
                return AuthoredReferenceHeadFact::Unavailable;
            };
            let mut parts = name.split('.').map(Arc::<str>::from);
            let Some(root) = parts.next() else {
                return AuthoredReferenceHeadFact::Unavailable;
            };
            let members = parts.collect::<Vec<_>>();
            if members.is_empty() {
                AuthoredReferenceHeadFact::Bare {
                    local_name: root,
                    args,
                }
            } else {
                AuthoredReferenceHeadFact::Qualified {
                    local_root: root,
                    member_path: Arc::from(members),
                    args,
                }
            }
        }
        TypeExpr::ImportType {
            specifier,
            qualifier,
            type_arguments,
            ..
        } => args(type_arguments.len()).map_or(AuthoredReferenceHeadFact::Unavailable, |args| {
            AuthoredReferenceHeadFact::ImportType {
                specifier: Arc::clone(specifier),
                member_path: Arc::clone(qualifier),
                args,
            }
        }),
        _ => AuthoredReferenceHeadFact::NotReference,
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
            false,
            "alias",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
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
            false,
            "alias",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
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
            false,
            "own",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
            None,
            None,
        );
        assert_eq!(fact.typeof_alias_target, None, "self-peel must break");
        assert_eq!(fact.classification, ValueAnnotationClass::Direct);
    }

    /// The producer contract for the authored reference head, pinned in EVERY
    /// direction so a future producer cannot leave the provenance inconsistent.
    ///
    /// [`value_type_annotation_fact`] is the SINGLE producer of the head for an
    /// authored value annotation. Two production sites construct the enclosing
    /// fact without it — `PreparedValueDecl::new`'s placeholder (immediately
    /// overwritten by preparation) and the SYNTHESISED component-default value
    /// decl — and both describe a declaration with NO authored annotation, for
    /// which `NotReference` is the CORRECT head, not a stub. This test pins the
    /// mapping that makes those two sites honest: reference ⇒ exact head,
    /// non-reference ⇒ `NotReference`, absent ⇒ `NotReference`,
    /// inference-refused ⇒ `Unavailable`. A producer that returned
    /// `NotReference` for an authored REFERENCE annotation fails here.
    #[test]
    fn the_reference_head_producer_is_exact_for_every_annotation_shape() {
        let canonical: Arc<str> = Arc::from("/ws/a.ts");
        let head_for = |annotation: Option<&TypeExpr>, unavailable| {
            value_type_annotation_fact(
                annotation,
                false,
                "subject",
                &canonical,
                TopLevelOwnerId::ordinary_file(),
                None,
                None,
                unavailable,
            )
            .reference_head
        };

        // A bare authored reference mints the EXACT authored local name — never
        // `NotReference`, and never the resolved target.
        let bare = TypeExpr::Ref {
            name: Arc::from("Alias"),
            type_arguments: Arc::from([TypeExpr::Primitive(PrimitiveName::String)]),
        };
        let AuthoredReferenceHeadFact::Bare { local_name, args } = head_for(Some(&bare), None)
        else {
            panic!("an authored bare reference must mint a `Bare` head");
        };
        assert_eq!(local_name.as_ref(), "Alias");
        assert_eq!(args.len(), 1, "one authored argument locator per type arg");

        // A dotted authored reference keeps the root AND the member path.
        let qualified = TypeExpr::Ref {
            name: Arc::from("Ns.Inner"),
            type_arguments: Arc::from([]),
        };
        let AuthoredReferenceHeadFact::Qualified {
            local_root,
            member_path,
            ..
        } = head_for(Some(&qualified), None)
        else {
            panic!("a dotted authored reference must mint a `Qualified` head");
        };
        assert_eq!(local_root.as_ref(), "Ns");
        assert_eq!(
            member_path.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
            ["Inner"]
        );

        // NEGATIVE half: a non-reference annotation, an ABSENT annotation, and
        // a refused inference are each their own typed state. `NotReference`
        // for an absent annotation is what makes the two non-producer
        // production construction sites correct rather than inconsistent.
        assert_eq!(
            head_for(Some(&TypeExpr::Primitive(PrimitiveName::String)), None),
            AuthoredReferenceHeadFact::NotReference,
            "a non-reference annotation has no authored reference head"
        );
        assert_eq!(
            head_for(None, None),
            AuthoredReferenceHeadFact::NotReference,
            "an ABSENT annotation has no authored reference head — the state \
             the synthesised-default and placeholder sites legitimately carry"
        );
        assert_eq!(
            head_for(None, Some(InferenceUnavailableReason::WorkBudgetExceeded)),
            AuthoredReferenceHeadFact::Unavailable,
            "a refused inference is UNAVAILABLE, never a claimed non-reference"
        );
        assert_ne!(
            AuthoredReferenceHeadFact::Unavailable,
            AuthoredReferenceHeadFact::NotReference,
            "unavailable and not-a-reference must stay distinguishable"
        );
    }

    #[test]
    fn absent_and_direct_annotations_classify_without_a_target() {
        let canonical: Arc<str> = Arc::from("/ws/a.ts");
        let absent = value_type_annotation_fact(
            None,
            false,
            "x",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            None,
            None,
            None,
        );
        assert_eq!(absent.classification, ValueAnnotationClass::Absent);
        assert_eq!(absent.typeof_alias_target, None);
        assert_eq!(absent.annotation, None);

        let direct = value_type_annotation_fact(
            Some(&TypeExpr::Primitive(PrimitiveName::String)),
            false,
            "x",
            &canonical,
            TopLevelOwnerId::ordinary_file(),
            Some(SemanticTypeSource::Closed(
                verter_type_expr::facts::ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                    PrimitiveName::String,
                )),
            )),
            None,
            None,
        );
        assert_eq!(direct.classification, ValueAnnotationClass::Direct);
        assert_eq!(direct.typeof_alias_target, None);
        assert!(direct.annotation.is_some(), "direct source is carried");
    }
}
