//! Compile-time marker witnesses + [P2] discrimination fixtures for the closed
//! fact / locator / span-origin families.
//!
//! Every fact, locator, and span-origin type is asserted `NoTypeExpr +
//! NoStoredSpan + Eq + Hash + Clone + Debug`. The marker SPLIT is proven
//! directly: `verter_span::Span` and `MemberSpans` are `NoTypeExpr` but NOT
//! `NoStoredSpan` — which is exactly why a fact must recover spans via a
//! producer-emitted origin locator rather than store one.
//!
//! The `#[test]` fixtures discriminate that the schema carries the named-required
//! metadata (`declared_in_macro_type_arg`, method optionality, member
//! visibility, index-key shape, tuple label/optional/rest): each builds two facts
//! that differ ONLY on the metadata axis and asserts inequality, plus an
//! identical pair asserting equality — never an always-true predicate.

use static_assertions::{assert_impl_all, assert_not_impl_any};

use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

use crate::facts::*;
use crate::locators::*;
use crate::span_origins::*;
use crate::{MemberSpans, MemberVisibility, PrimitiveName};

/// Assert every listed type is a fully-witnessed closed fact carrier.
macro_rules! assert_fact_carriers {
    ($($ty:ty),+ $(,)?) => {
        $(
            assert_impl_all!(
                $ty: NoTypeExpr,
                NoStoredSpan,
                ::core::cmp::Eq,
                ::core::hash::Hash,
                ::core::clone::Clone,
                ::core::fmt::Debug
            );
        )+
    };
}

// --- Locators ---
assert_fact_carriers!(
    LocatorSymbolSpace,
    AuthoredAnchor,
    TypeBodyPathStep,
    TypeBodySlot,
    SymbolBodyLocator,
    TypeArgLocator,
    MacroPayloadPosition,
    MacroPayloadLocator,
    AuthoredBodyLocator,
);

// --- Span origins ---
assert_fact_carriers!(
    DeclContributorAnchor,
    SourceSynthetic,
    MemberSpansOrigin,
    IndexSignatureSpansOrigin,
    FunctionSpansOrigin,
    FunctionParamSelector,
    FunctionParamSpanOrigin,
);

// --- Supporting new types + Surface A ---
assert_fact_carriers!(
    DeclarationOrigin,
    ValueDeclIdentityPart,
    HeritageBaseFact,
    ClosednessFollowRole,
    SymbolicBinding,
    SymbolicBindingLocator,
    FollowLocatorPayload,
    ClosednessRecipe,
    KeyDomainFact,
);

// --- Surface B ---
assert_fact_carriers!(
    NarrowFrontierBody,
    MemberNamesRoute,
    MemberDependencyEdge,
    ShallowRouteFacts,
    ValueAnnotationClass,
    ValueTypeAnnotationFact,
    NarrowTypeParam,
    TypeParamDeclFact,
);

// --- Surface C ---
assert_fact_carriers!(
    TypeBodyClass,
    PreparedTypeBodyFacts,
    FunctionParamFact,
    FunctionSignatureFact,
    ObjectPropertyFact,
    ObjectMethodFact,
    KeyTypeShape,
    IndexSignatureFact,
    ObjectMemberFact,
    ObjectShapeFact,
    EnumScalar,
    EnumPrimitiveDomain,
    EnumMemberEntry,
    EnumMemberFact,
    PreparedMemberFact,
    PreparedValueMemberFact,
    PreparedCaseTransformKind,
    PreparedKeyFilterShapeFact,
    PreparedKeyRemapShapeFact,
    PreparedValueRuleShapeFact,
    PreparedForwardingKind,
    PreparedForwardPayloadFact,
    PreparedWrapperKindFact,
    PreparedSurfaceModifiersFact,
    PreparedWrapperShapeFact,
    PreparedProjectionClassFact,
);

// --- Analyzed* / Projected* / synthesized / Svelte ---
assert_fact_carriers!(
    AnalyzedMacroKindFact,
    AnalyzedPropFieldFact,
    AnalyzedEmitFieldFact,
    AnalyzedSlotFieldBindingFact,
    AnalyzedSlotFieldFact,
    AnalyzedOptionsPropFact,
    AnalyzedExposeFieldFact,
    AnalyzedMacroFact,
    FactOrLocator,
    LeafTypeFact,
    SynthesizedMemberFact,
    TupleElementFact,
    TuplePayloadFact,
    IndexedAccessFact,
    ResolvedLocalShape,
    ResolvedLocalTypeFact,
    ProjectedMemberFact,
    ProjectedIndexSignatureFact,
    ProjectedSurfaceFact,
    SvelteLegacyPropFact,
    SvelteScriptFactsFact,
);

// --- The marker SPLIT (the whole reason NoStoredSpan is a separate marker) ---
// A span IS NoTypeExpr but is NOT NoStoredSpan; MemberSpans (span-only) mirrors
// it. This is what forces facts to recover-via-locator instead of storing a span.
assert_impl_all!(verter_span::Span: NoTypeExpr);
assert_not_impl_any!(verter_span::Span: NoStoredSpan);
assert_impl_all!(MemberSpans: NoTypeExpr);
assert_not_impl_any!(MemberSpans: NoStoredSpan);
assert_not_impl_any!(Option<verter_span::Span>: NoStoredSpan);
// A fact never even NAMES TypeExpr; assert the danger sibling stays non-marker
// so the split can never regress into over-storing a body.
assert_not_impl_any!(crate::TypeExpr: NoTypeExpr);

// A closed fact is never a witness that a span slipped in: assert the strongest
// carriers (those that recover spans) genuinely satisfy NoStoredSpan.
assert_impl_all!(ObjectShapeFact: NoStoredSpan);
assert_impl_all!(FunctionSignatureFact: NoStoredSpan);
assert_impl_all!(ResolvedLocalTypeFact: NoStoredSpan);

// ---------------------------------------------------------------------------
// [P2] discrimination fixtures
// ---------------------------------------------------------------------------

fn anchor() -> AuthoredAnchor {
    AuthoredAnchor {
        canonical_id: std::sync::Arc::from("/ws/a.ts"),
        symbol: std::sync::Arc::from("A"),
        space: LocatorSymbolSpace::Type,
    }
}

fn slot() -> TypeBodySlot {
    TypeBodySlot {
        anchor: anchor(),
        path: std::sync::Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
    }
}

fn member_origin(ordinal: u32) -> MemberSpansOrigin {
    MemberSpansOrigin::Authored {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
        },
        member_path: std::sync::Arc::from(vec![ordinal].into_boxed_slice()),
    }
}

fn prop(name: &str, optional: bool, visibility: MemberVisibility) -> ObjectPropertyFact {
    ObjectPropertyFact {
        name: name.to_string(),
        optional,
        readonly: false,
        visibility,
        ty: slot(),
        span_origin: member_origin(0),
    }
}

fn empty_fn() -> FunctionSignatureFact {
    FunctionSignatureFact {
        type_parameters: std::sync::Arc::from(Vec::<NarrowTypeParam>::new().into_boxed_slice()),
        parameters: std::sync::Arc::from(Vec::<FunctionParamFact>::new().into_boxed_slice()),
        return_ty: None,
        has_implementation_body: false,
        spans_origin: FunctionSpansOrigin::AliasBody {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
            },
        },
    }
}

fn method(name: &str, optional: bool) -> ObjectMethodFact {
    ObjectMethodFact {
        name: name.to_string(),
        optional,
        visibility: MemberVisibility::Public,
        function: empty_fn(),
        span_origin: member_origin(0),
    }
}

fn index_sig(key: KeyTypeShape) -> IndexSignatureFact {
    IndexSignatureFact {
        key_name: "k".to_string(),
        key_type: key,
        value_type: slot(),
        readonly: false,
        span_origin: IndexSignatureSpansOrigin::Authored {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
            },
            member_path: std::sync::Arc::from(vec![0u32].into_boxed_slice()),
        },
    }
}

fn tuple_element(label: Option<&str>, optional: bool, rest: bool) -> TupleElementFact {
    TupleElementFact {
        label: label.map(str::to_string),
        optional,
        rest,
        ty: FactOrLocator::Leaf(LeafTypeFact::Primitive(PrimitiveName::String)),
    }
}

fn prop_field(name: &str, declared_in_macro_type_arg: bool) -> AnalyzedPropFieldFact {
    AnalyzedPropFieldFact {
        name: name.to_string(),
        is_optional: false,
        declared_in_macro_type_arg,
        type_expr_scope: None,
        payload: None,
        name_span_origin: member_origin(0),
    }
}

#[test]
fn declared_in_macro_type_arg_participates_in_fact_identity() {
    // The policy-consumed flag MUST be part of fact identity — a prop declared in
    // the macro type argument is a DISTINCT fact from a heritage-derived one, so a
    // publication filter can never collapse them.
    let authored = prop_field("count", true);
    let heritage = prop_field("count", false);
    assert_ne!(
        authored, heritage,
        "declared_in_macro_type_arg must discriminate fact identity"
    );
    // ...but two facts that agree on the flag (and everything else) are equal.
    assert_eq!(authored, prop_field("count", true));
}

#[test]
fn method_optionality_participates_in_fact_identity() {
    let required = method("run", false);
    let optional = method("run", true);
    assert_ne!(required, optional, "method optionality must discriminate");
    assert_eq!(required, method("run", false));
}

#[test]
fn member_visibility_participates_in_fact_identity() {
    let public = prop("x", false, MemberVisibility::Public);
    let protected = prop("x", false, MemberVisibility::Protected);
    let private = prop("x", false, MemberVisibility::Private);
    // All three visibilities are distinct fact identities (publication filters at
    // the boundary, never by collapsing identity).
    assert_ne!(public, protected);
    assert_ne!(protected, private);
    assert_ne!(public, private);
    assert_eq!(public, prop("x", false, MemberVisibility::Public));
}

#[test]
fn index_key_shape_participates_in_fact_identity() {
    // `[k: string]` and `[k: number]` are DISTINCT index-signature facts — the
    // declared key SHAPE is preserved, so they never alias.
    let string_key = index_sig(KeyTypeShape::String);
    let number_key = index_sig(KeyTypeShape::Number);
    assert_ne!(string_key, number_key, "index key shape must discriminate");
    assert_eq!(string_key, index_sig(KeyTypeShape::String));
}

#[test]
fn tuple_element_label_optional_rest_participate_in_fact_identity() {
    let base = tuple_element(Some("a"), false, false);
    // Each of label / optional / rest must independently discriminate.
    assert_ne!(base, tuple_element(Some("b"), false, false), "label");
    assert_ne!(base, tuple_element(Some("a"), true, false), "optional");
    assert_ne!(base, tuple_element(Some("a"), false, true), "rest");
    assert_ne!(
        base,
        tuple_element(None, false, false),
        "labelled vs unlabelled"
    );
    assert_eq!(base, tuple_element(Some("a"), false, false));
}

#[test]
fn closedness_recipe_intersection_arm_is_structurally_recursive() {
    // The self-referential composition arm carries nested recipes and stays
    // sound under Eq/Hash — a discriminating check that the hand-written witness
    // did not flatten the arm away.
    let inner = ClosednessRecipe::ObjectClosed;
    let outer = ClosednessRecipe::IntersectionAllArms(std::sync::Arc::from(
        vec![inner.clone(), ClosednessRecipe::MappedOpenParam].into_boxed_slice(),
    ));
    let outer_same = ClosednessRecipe::IntersectionAllArms(std::sync::Arc::from(
        vec![
            ClosednessRecipe::ObjectClosed,
            ClosednessRecipe::MappedOpenParam,
        ]
        .into_boxed_slice(),
    ));
    let outer_diff =
        ClosednessRecipe::IntersectionAllArms(std::sync::Arc::from(vec![inner].into_boxed_slice()));
    assert_eq!(outer, outer_same);
    assert_ne!(outer, outer_diff, "arm contents must discriminate");
}
