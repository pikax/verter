//! Discrimination tests for `SemanticTypeSource::absolutized_against` — the
//! cross-owner self-anchoring normalizer. A producer-local (empty
//! `canonical_id`) anchor absolutizes to the supplied owning canonical at
//! EVERY nesting depth; an already-absolute anchor is NEVER rewritten; the
//! anchor-free closed leaf family passes through untouched (a published
//! child-local alias name stays the name AS WRITTEN).
//!
//! Each test discriminates: it FAILS against a tree without the deep
//! normalizer (compile-missing or a shallow-only rewrite) AND PASSES with it.

use std::sync::Arc;

use verter_type_expr::facts::DeclarationOrigin;
use verter_type_expr::facts::{
    ClosedTypeFact, FactOrLocator, FunctionParamFact, FunctionSignatureFact, LeafTypeFact,
    ObjectMemberFact, ObjectPropertyFact, ObjectShapeFact, ProjectedMemberFact, ProjectedTypeFact,
    ResolvedLocalShape, SemanticTypeSource, SynthesizedMemberFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadLocator,
    MacroPayloadPosition, TypeBodySlot,
};
use verter_type_expr::span_origins::{
    FunctionParamSelector, FunctionParamSpanOrigin, FunctionSpansOrigin, MemberSpansOrigin,
    SourceSynthetic,
};
use verter_type_expr::{MemberVisibility, TopLevelOwnerId};

const CHILD: &str = "/components/Child.vue";

fn anchor(canonical: &str) -> AuthoredAnchor {
    AuthoredAnchor {
        canonical_id: Arc::from(canonical),
        owner: TopLevelOwnerId::ordinary_file(),
        symbol: Arc::from("ChildProps"),
        space: LocatorSymbolSpace::Type,
    }
}

fn slot(canonical: &str) -> TypeBodySlot {
    TypeBodySlot {
        anchor: anchor(canonical),
        path: Arc::from(Vec::new().into_boxed_slice()),
    }
}

#[test]
fn producer_local_authored_decl_body_anchor_absolutizes() {
    let source = SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(slot("")));
    let normalized = source.absolutized_against(CHILD);
    let SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(rewritten)) = &normalized else {
        panic!("normalizer must preserve the source arm, got {normalized:?}");
    };
    assert_eq!(
        rewritten.anchor.canonical_id.as_ref(),
        CHILD,
        "an empty (producer-local) anchor must absolutize to the owning canonical"
    );
    assert_eq!(
        rewritten.anchor.symbol.as_ref(),
        "ChildProps",
        "absolutization rewrites ONLY the canonical id"
    );
}

#[test]
fn absolute_anchor_is_never_rewritten() {
    let source =
        SemanticTypeSource::Authored(AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: anchor("/lib/other.ts"),
            macro_index: 2,
            payload: MacroPayloadPosition::TypeArgument,
        }));
    let normalized = source.absolutized_against(CHILD);
    assert_eq!(
        normalized, source,
        "an already-absolute anchor must survive normalization byte-identically"
    );
}

#[test]
fn closed_leaf_and_leaf_union_pass_through_untouched() {
    let leaf = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(
        "ChildLocalAlias".to_string(),
    )));
    assert_eq!(
        leaf.absolutized_against(CHILD),
        leaf,
        "a closed leaf ref keeps the name AS WRITTEN — no anchor to rewrite"
    );
    let union = SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(Arc::from(
        vec![
            LeafTypeFact::StringLiteral("a".to_string()),
            LeafTypeFact::NumberLiteral("1".to_string()),
        ]
        .into_boxed_slice(),
    )));
    assert_eq!(union.absolutized_against(CHILD), union);
}

#[test]
fn synthesized_object_member_locator_absolutizes_deeply() {
    let source = SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(Arc::from(
        vec![SynthesizedMemberFact {
            name: "value".to_string(),
            optional: false,
            ty: FactOrLocator::Locator(slot("")),
            span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
        }]
        .into_boxed_slice(),
    )));
    let normalized = source.absolutized_against(CHILD);
    let SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(members)) = &normalized else {
        panic!("normalizer must preserve the synthesized object arm");
    };
    let FactOrLocator::Locator(rewritten) = &members[0].ty else {
        panic!("member ty must stay the locator arm");
    };
    assert_eq!(
        rewritten.anchor.canonical_id.as_ref(),
        CHILD,
        "a NESTED producer-local locator must absolutize (deep walk, not shallow)"
    );
}

#[test]
fn closed_object_property_slot_absolutizes_deeply() {
    let source = SemanticTypeSource::Closed(ClosedTypeFact::Object(ObjectShapeFact {
        members: Arc::from(
            vec![ObjectMemberFact::Property(ObjectPropertyFact {
                name: "x".to_string(),
                optional: false,
                readonly: false,
                visibility: MemberVisibility::Public,
                ty: slot(""),
                span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
            })]
            .into_boxed_slice(),
        ),
    }));
    let normalized = source.absolutized_against(CHILD);
    let SemanticTypeSource::Closed(ClosedTypeFact::Object(shape)) = &normalized else {
        panic!("normalizer must preserve the closed object arm");
    };
    let ObjectMemberFact::Property(property) = &shape.members[0] else {
        panic!("member must stay a property");
    };
    assert_eq!(property.ty.anchor.canonical_id.as_ref(), CHILD);
}

#[test]
fn projected_member_and_function_positions_absolutize_deeply() {
    let member = SemanticTypeSource::Projected(ProjectedTypeFact::Member(ProjectedMemberFact {
        name: "onSelect".to_string(),
        optional: true,
        readonly: false,
        is_method: false,
        visibility: MemberVisibility::Public,
        declared_in_macro_type_arg: false,
        declaration_origin: DeclarationOrigin::Synthetic,
        ty: slot(""),
        span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
    }));
    let normalized = member.absolutized_against(CHILD);
    let SemanticTypeSource::Projected(ProjectedTypeFact::Member(rewritten)) = &normalized else {
        panic!("normalizer must preserve the projected member arm");
    };
    assert_eq!(rewritten.ty.anchor.canonical_id.as_ref(), CHILD);

    let call =
        SemanticTypeSource::Projected(ProjectedTypeFact::CallSignature(FunctionSignatureFact {
            type_parameters: Arc::from(Vec::new().into_boxed_slice()),
            parameters: Arc::from(
                vec![FunctionParamFact {
                    name: Some("payload".to_string()),
                    optional: false,
                    rest: false,
                    has_ts_annotation: true,
                    ty: Some(slot("")),
                    span_origin: FunctionParamSpanOrigin {
                        function: FunctionSpansOrigin::Synthetic(SourceSynthetic),
                        param: FunctionParamSelector::Positional { ordinal: 0 },
                    },
                }]
                .into_boxed_slice(),
            ),
            return_ty: Some(slot("/lib/keep.ts")),
            return_inference: ReturnInferenceCompleteness::NotInferred,
            has_implementation_body: false,
            spans_origin: FunctionSpansOrigin::Synthetic(SourceSynthetic),
        }));
    let normalized = call.absolutized_against(CHILD);
    let SemanticTypeSource::Projected(ProjectedTypeFact::CallSignature(signature)) = &normalized
    else {
        panic!("normalizer must preserve the call-signature arm");
    };
    let param_slot = signature.parameters[0]
        .ty
        .as_ref()
        .expect("the annotated param keeps its slot");
    assert_eq!(
        param_slot.anchor.canonical_id.as_ref(),
        CHILD,
        "a producer-local function-parameter slot must absolutize"
    );
    assert_eq!(
        signature
            .return_ty
            .as_ref()
            .expect("return slot kept")
            .anchor
            .canonical_id
            .as_ref(),
        "/lib/keep.ts",
        "an absolute return-type anchor must NOT be rewritten"
    );
}

#[test]
fn synthetic_slot_binding_carrier_is_untouched() {
    let source =
        SemanticTypeSource::SyntheticSlotBinding(Arc::new(verter_type_expr::SyntheticCarrierKey {
            scope_canonical_id: Arc::from(CHILD),
            surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("row"),
            value_node: 7,
        }));
    assert_eq!(
        source.absolutized_against("/pages/Parent.vue"),
        source,
        "the synthetic binding key carries its own scope — never rewritten"
    );
}
