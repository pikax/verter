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
use crate::intrinsics::*;
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
    TypeParamBoundPosition,
    TypeBodyPathStep,
    TypeBodySlot,
    SymbolBodyLocator,
    TypeArgLocator,
    MacroPayloadPosition,
    MacroPayloadLocator,
    AuthoredBodyLocator,
    AuthoredTypePayloadRef,
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
    KeyDomainClosednessFact,
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

// --- Route-demand + decl-header inventory facts ---
assert_fact_carriers!(
    RouteDemand,
    RouteKeySet,
    ExternalRouteRefFact,
    RouteDependencyRefFact,
    WholeRouteContextFact,
    WholeRouteEdgeFact,
    DeferredKeyUtilityKind,
    DeferredKeyUtilityEdge,
    MemberPathSeedTarget,
    MemberPathSeedEdge,
    MemberHeaderFact,
    EnumMemberNamesFact,
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
    SvelteModuleExportFact,
    SvelteScriptFactsFact,
);

// --- Four-source SemanticTypeSource + expansion-result wrapper + intrinsics ---
assert_fact_carriers!(
    ClosedTypeFact,
    ProjectedTypeFact,
    IndexSignaturePosition,
    // `SynthesizedTypeFact` is a type alias for `ResolvedLocalShape` (reused, not
    // duplicated); asserting it documents the alias is a witnessed carrier.
    SynthesizedTypeFact,
    SemanticTypeSource,
    // The three-state source-POSITION carrier and its two leaf reason enums:
    // schema-absence, present source, and source-construction failure are
    // distinct typed states — the carrier is a fact citizen like the source
    // it wraps.
    SourcePosition,
    SchemaAbsence,
    SemanticSourceFailure,
    ExpansionExactnessFact,
    ExpansionExecutionStatusFact,
    // The generic wrapper is a carrier for a concrete fact payload; the derived
    // witnesses forward the marker bound to `T`.
    ExpansionResultFact<ObjectShapeFact>,
    JsdocTypedefBodyLocator,
    StaticIntrinsicTypeId,
    IntrinsicMemberKind,
    IntrinsicMemberFact,
);

// The generic expansion wrapper REJECTS a raw-`TypeExpr` payload: the derived
// witnesses forward the marker bound to the payload, so `ExpansionResultFact<T>`
// is `NoTypeExpr`/`NoStoredSpan` iff `T` is — and `TypeExpr` is neither. This is
// the structural rejection proof for the new generic carrier (a compile-time
// witness in the existing `assert_not_impl_any!` pattern, NOT a name scanner):
// it FAILS TO COMPILE if the wrapper ever became unconditionally a carrier.
assert_not_impl_any!(ExpansionResultFact<crate::TypeExpr>: NoTypeExpr, NoStoredSpan);

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
        owner: crate::TopLevelOwnerId::ordinary_file(),
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
            owner: crate::TopLevelOwnerId::ordinary_file(),
            owner_local_ordinal: 0,
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
        return_inference: ReturnInferenceCompleteness::NotInferred,
        has_implementation_body: false,
        spans_origin: FunctionSpansOrigin::AliasBody {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
                owner: crate::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
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
                owner: crate::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
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

/// Deterministic RECORDING `Hasher`: captures the exact byte stream fed
/// through [`std::hash::Hasher::write`] (every `write_u*` / `write_str` /
/// length-prefix default routes through it) so a witness can assert on the
/// HASH INPUT STREAM itself. A folded-digest or set-membership assert
/// resolves collisions via `Eq` and therefore passes even under a constant
/// `Hash`; the recorded stream discriminates that degenerate class directly.
#[derive(Default)]
struct RecordingHasher {
    stream: Vec<u8>,
}

impl std::hash::Hasher for RecordingHasher {
    fn finish(&self) -> u64 {
        // The digest is deliberately NOT the observable — witnesses compare
        // the recorded input stream, never a folded value.
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.stream.extend_from_slice(bytes);
    }
}

/// The exact hash INPUT stream `value` feeds into a `Hasher`.
fn hash_input_stream<T: std::hash::Hash>(value: &T) -> Vec<u8> {
    let mut hasher = RecordingHasher::default();
    value.hash(&mut hasher);
    hasher.stream
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
fn fact_or_locator_leaf_union_is_a_closed_ordered_carrier() {
    let leaves = |kinds: &[PrimitiveName]| -> FactOrLocator {
        FactOrLocator::LeafUnion(std::sync::Arc::from(
            kinds
                .iter()
                .map(|kind| LeafTypeFact::Primitive(*kind))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ))
    };
    let string_number = leaves(&[PrimitiveName::String, PrimitiveName::Number]);
    let number_string = leaves(&[PrimitiveName::Number, PrimitiveName::String]);

    // Ordered as produced: the member ORDER participates in fact identity.
    assert_ne!(
        string_number, number_string,
        "leaf-union member order must discriminate fact identity"
    );
    assert_eq!(
        string_number,
        leaves(&[PrimitiveName::String, PrimitiveName::Number])
    );
    // The union arm never aliases a single leaf over the same primitive.
    assert_ne!(
        string_number,
        FactOrLocator::Leaf(LeafTypeFact::Primitive(PrimitiveName::String)),
        "a leaf union must not alias a bare leaf"
    );
    // Hash identity agrees — asserted on the HASH INPUT STREAMS through the
    // recording hasher, never a set length (a `HashSet` membership count
    // resolves collisions via `Eq`, so three distinct carriers make three
    // entries even under a constant `Hash`). The arm TAG (`LeafUnion` vs
    // `Leaf`) and the ordered leaf members must each feed distinct bytes.
    let bare_leaf = FactOrLocator::Leaf(LeafTypeFact::Primitive(PrimitiveName::String));
    let stream_string_number = hash_input_stream(&string_number);
    let stream_number_string = hash_input_stream(&number_string);
    let stream_bare_leaf = hash_input_stream(&bare_leaf);
    assert_ne!(
        stream_string_number, stream_number_string,
        "leaf-union member ORDER must feed distinct hash input streams"
    );
    assert_ne!(
        stream_string_number, stream_bare_leaf,
        "the LeafUnion arm tag must feed a distinct hash input stream from a bare Leaf"
    );
    assert_ne!(
        stream_number_string, stream_bare_leaf,
        "the LeafUnion arm tag must feed a distinct hash input stream from a bare Leaf"
    );
    // Determinism: an equal carrier feeds the identical input stream.
    assert_eq!(
        stream_string_number,
        hash_input_stream(&leaves(&[PrimitiveName::String, PrimitiveName::Number])),
        "equal carriers must feed identical hash input streams"
    );

    // Serde round-trip in the NESTED carrier position (a labelled tuple
    // element — the realized emit payload shape) is identity.
    let element = TupleElementFact {
        label: Some("payload".to_string()),
        optional: false,
        rest: false,
        ty: string_number.clone(),
    };
    let json = serde_json::to_string(&element).expect("serialize");
    let back: TupleElementFact = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, element, "leaf-union tuple element must round-trip");
    assert_eq!(back.ty, string_number);
    // A leaf-union element is a distinct fact identity from a bare-leaf
    // element at the same position.
    assert_ne!(
        back,
        TupleElementFact {
            ty: FactOrLocator::Leaf(LeafTypeFact::Primitive(PrimitiveName::String)),
            ..element
        }
    );
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
fn type_param_bound_position_and_ordinal_participate_in_locator_identity() {
    // A type-parameter bound locator's identity spans BOTH axes: which
    // parameter (`ordinal`) and which authored bound slot (`position`). Two
    // slots differing on ONLY one axis must be distinct content-free identities,
    // so a constraint locator can never alias its sibling default (or a
    // different parameter's bound).
    let bound = |ordinal: u32, position: TypeParamBoundPosition| TypeBodySlot {
        anchor: anchor(),
        path: std::sync::Arc::from(
            vec![TypeBodyPathStep::TypeParamBound { ordinal, position }].into_boxed_slice(),
        ),
    };
    let constraint0 = bound(0, TypeParamBoundPosition::Constraint);
    let default0 = bound(0, TypeParamBoundPosition::Default);
    let constraint1 = bound(1, TypeParamBoundPosition::Constraint);
    assert_ne!(
        constraint0, default0,
        "constraint vs default must discriminate a bound locator"
    );
    assert_ne!(
        constraint0, constraint1,
        "the parameter ordinal must discriminate a bound locator"
    );
    assert_eq!(
        constraint0,
        bound(0, TypeParamBoundPosition::Constraint),
        "two bound locators agreeing on both axes are the same identity"
    );
}

#[test]
fn heritage_base_fact_name_and_arg_locators_participate_in_identity() {
    // The producer-minted class-heritage fact must discriminate on every
    // authored axis: the base NAME, the `name_resolution` routing key, and
    // each type-argument locator (position path + arg ordinal). A resolved
    // identity is NEVER stored — two facts naming the same resolved base
    // through different authored heads stay distinct.
    let arg = |arm: u32, arg_index: u32| TypeArgLocator {
        anchor: anchor(),
        path: std::sync::Arc::from(
            vec![TypeBodyPathStep::IntersectionArm { ordinal: arm }].into_boxed_slice(),
        ),
        arg_index,
    };
    let fact = |name: &str, args: Vec<TypeArgLocator>| HeritageBaseFact {
        name: name.to_string(),
        type_args: std::sync::Arc::from(args.into_boxed_slice()),
        name_resolution_ref: name.to_string(),
        base_name_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
    };
    let base = fact("Base", vec![arg(0, 0)]);
    assert_ne!(
        base,
        fact("Other", vec![arg(0, 0)]),
        "the authored base name must discriminate"
    );
    assert_ne!(
        base,
        fact("Base", vec![arg(0, 1)]),
        "the argument ordinal must discriminate"
    );
    assert_ne!(
        base,
        fact("Base", vec![arg(1, 0)]),
        "the arg-bearing position (intersection arm) must discriminate"
    );
    assert_ne!(
        base,
        fact("Base", Vec::new()),
        "an arg-less base is distinct from an instantiated one"
    );
    assert_ne!(
        base,
        HeritageBaseFact {
            name_resolution_ref: "Aliased".to_string(),
            ..base.clone()
        },
        "the name_resolution routing key must discriminate"
    );
    assert_eq!(base, fact("Base", vec![arg(0, 0)]));
}

#[test]
fn closedness_recipe_all_arms_is_structurally_recursive() {
    // The self-referential composition arm carries nested recipes and stays
    // sound under Eq/Hash — a discriminating check that the hand-written witness
    // did not flatten the arm away.
    let inner = ClosednessRecipe::ObjectClosed;
    let outer = ClosednessRecipe::AllArms(std::sync::Arc::from(
        vec![inner.clone(), ClosednessRecipe::OpenLeaf].into_boxed_slice(),
    ));
    let outer_same = ClosednessRecipe::AllArms(std::sync::Arc::from(
        vec![ClosednessRecipe::ObjectClosed, ClosednessRecipe::OpenLeaf].into_boxed_slice(),
    ));
    let outer_diff =
        ClosednessRecipe::AllArms(std::sync::Arc::from(vec![inner].into_boxed_slice()));
    assert_eq!(outer, outer_same);
    assert_ne!(outer, outer_diff, "arm contents must discriminate");
}

fn empty_projected_surface() -> ProjectedSurfaceFact {
    ProjectedSurfaceFact {
        members: std::sync::Arc::from(Vec::<ProjectedMemberFact>::new().into_boxed_slice()),
        call_signatures: std::sync::Arc::from(
            Vec::<FunctionSignatureFact>::new().into_boxed_slice(),
        ),
        construct_signatures: std::sync::Arc::from(
            Vec::<FunctionSignatureFact>::new().into_boxed_slice(),
        ),
        index_signatures: std::sync::Arc::from(
            Vec::<ProjectedIndexSignatureFact>::new().into_boxed_slice(),
        ),
        has_index_signature: false,
    }
}

#[test]
fn enum_primitive_domain_carries_four_distinct_arms() {
    use EnumPrimitiveDomain::*;
    // The two NEW arms (`NumberOrString`, `Unknown`) are constructible and each
    // distinct from BOTH originals and from each other — the whole reason the fix
    // exists: a deferred `+` member (number | string) and a genuinely unprovable
    // member cannot faithfully collapse into `Number` or `String` alone.
    let all = [Number, String, NumberOrString, Unknown];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b, "every EnumPrimitiveDomain arm must discriminate");
            }
        }
    }
    // Hash identity too (the domain participates in enum-member fact identity):
    // four arms must be four distinct set entries.
    let set: std::collections::HashSet<EnumPrimitiveDomain> = all.iter().copied().collect();
    assert_eq!(set.len(), 4);
}

#[test]
fn semantic_type_source_four_sources_construct_and_discriminate() {
    // One of each of the four disjoint sources. A resolved position's source must
    // never alias across the four-source model, even when inner facts coincide.
    let authored = SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(slot()));
    let closed_string = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
        PrimitiveName::String,
    )));
    let closed_number = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
        PrimitiveName::Number,
    )));
    // Same inner leaf as `closed_string`, but under the Synthesized source tag.
    let synthesized = SemanticTypeSource::Synthesized(ResolvedLocalShape::Leaf(
        LeafTypeFact::Primitive(PrimitiveName::String),
    ));
    let projected =
        SemanticTypeSource::Projected(ProjectedTypeFact::Surface(empty_projected_surface()));

    // Cross-source distinctness (all four sources are mutually distinct).
    let sources = [
        authored.clone(),
        closed_string.clone(),
        synthesized.clone(),
        projected.clone(),
    ];
    for (i, a) in sources.iter().enumerate() {
        for (j, b) in sources.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "the four sources must be mutually distinct");
            }
        }
    }
    // The SOURCE TAG discriminates even when the inner leaf is identical:
    // `Closed(Leaf(string))` != `Synthesized(Leaf(string))`.
    assert_ne!(
        closed_string, synthesized,
        "the source tag must discriminate identical inner leaves"
    );
    // Within a source, the inner fact discriminates.
    assert_ne!(closed_string, closed_number);
    assert_eq!(
        closed_string,
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            PrimitiveName::String
        )))
    );
    // Hash identity: four distinct sources → four set entries.
    let set: std::collections::HashSet<SemanticTypeSource> = sources.into_iter().collect();
    assert_eq!(set.len(), 4);
}

#[test]
fn expansion_result_fact_discriminates_metadata_and_payload() {
    let mk = |exactness, execution_status, prim| ExpansionResultFact {
        value: ClosedTypeFact::Leaf(LeafTypeFact::Primitive(prim)),
        exactness,
        execution_status,
    };
    let base = mk(
        ExpansionExactnessFact::ExactConcrete,
        ExpansionExecutionStatusFact::Completed,
        PrimitiveName::String,
    );
    // Each of exactness / execution_status / payload independently discriminates
    // (all three participate in fact identity; diagnostics were dropped and so
    // cannot participate).
    assert_ne!(
        base,
        mk(
            ExpansionExactnessFact::Incomplete,
            ExpansionExecutionStatusFact::Completed,
            PrimitiveName::String,
        ),
        "exactness must discriminate"
    );
    assert_ne!(
        base,
        mk(
            ExpansionExactnessFact::ExactConcrete,
            ExpansionExecutionStatusFact::Cancelled,
            PrimitiveName::String,
        ),
        "execution_status must discriminate"
    );
    assert_ne!(
        base,
        mk(
            ExpansionExactnessFact::ExactConcrete,
            ExpansionExecutionStatusFact::Completed,
            PrimitiveName::Number,
        ),
        "payload must discriminate"
    );
    assert_eq!(
        base,
        mk(
            ExpansionExactnessFact::ExactConcrete,
            ExpansionExecutionStatusFact::Completed,
            PrimitiveName::String,
        )
    );
}

#[test]
fn authored_type_payload_ref_discriminates_on_both_axes() {
    // The payload ref's identity spans BOTH axes: the authored POSITION
    // (locator) and the authored CONTENT (payload_hash). Two refs differing on
    // only one axis are distinct — a content edit at the same position must
    // never alias, and the same content at two positions must never alias.
    let mk = |macro_index: u32, payload: MacroPayloadPosition, hash: u8| AuthoredTypePayloadRef {
        locator: AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: anchor(),
            macro_index,
            payload,
        }),
        payload_hash: [hash; 16],
    };
    let base = mk(0, MacroPayloadPosition::TypeArgument, 0x11);
    assert_ne!(
        base,
        mk(0, MacroPayloadPosition::TypeArgument, 0x22),
        "the payload hash (authored content) must discriminate"
    );
    assert_ne!(
        base,
        mk(1, MacroPayloadPosition::TypeArgument, 0x11),
        "the macro ordinal (authored position) must discriminate"
    );
    assert_ne!(
        base,
        mk(0, MacroPayloadPosition::TypeAnnotation, 0x11),
        "the payload position must discriminate"
    );
    assert_eq!(base, mk(0, MacroPayloadPosition::TypeArgument, 0x11));

    // The new `TypeAnnotation` position is a distinct content-free position
    // from every prior arm (set cardinality proof, mirroring the path-step
    // fixture).
    let all = [
        MacroPayloadPosition::TypeArgument,
        MacroPayloadPosition::ObjectArgument,
        MacroPayloadPosition::Field { field_index: 0 },
        MacroPayloadPosition::TypeAnnotation,
    ];
    let set: std::collections::HashSet<MacroPayloadPosition> = all.iter().copied().collect();
    assert_eq!(set.len(), all.len(), "every payload position is distinct");
}

#[test]
fn jsdoc_typedef_body_is_a_distinct_authored_source_from_the_tstype_decl_body() {
    // A JSDoc `@typedef` body and a `TSType` decl body at the SAME anchor + empty
    // path are DISTINCT authored sources — the named sub-kind never aliases the
    // `TSType` decl-body arm (they deref differently: comment-text re-parse via
    // `parse_jsdoc_tag_type_payload` vs the retained `TSType`). This proves the
    // arm keeps the closed sum meaningfully closed rather than collapsing into
    // `DeclBody`.
    let jsdoc = AuthoredBodyLocator::JsdocTypedefBody(JsdocTypedefBodyLocator {
        anchor: anchor(),
        path: std::sync::Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
    });
    let decl = AuthoredBodyLocator::DeclBody(slot());
    assert_ne!(
        jsdoc, decl,
        "JSDoc typedef body must not alias the TSType decl body at the same anchor"
    );
    assert_eq!(jsdoc.clone(), jsdoc);
}

#[test]
fn new_type_body_path_step_arms_construct_and_discriminate() {
    use TypeBodyPathStep::*;
    // Ordinal-bearing arms: the ordinal participates in identity — a step at
    // ordinal 0 is a DISTINCT authored position from the same arm at ordinal 1.
    // (These lines would fail to COMPILE — and so fail the test — if any of
    // these arms ever dropped its ordinal to a nullary arm.)
    assert_ne!(FunctionParam { ordinal: 0 }, FunctionParam { ordinal: 1 });
    assert_ne!(ValueSignature { ordinal: 0 }, ValueSignature { ordinal: 1 });
    assert_ne!(UnionArm { ordinal: 0 }, UnionArm { ordinal: 1 });
    assert_ne!(TupleElement { ordinal: 0 }, TupleElement { ordinal: 1 });
    // ...same ordinal ⇒ same position.
    assert_eq!(FunctionParam { ordinal: 2 }, FunctionParam { ordinal: 2 });

    // Every arm (existing 5 + new 16) is a DISTINCT content-free position: a set
    // with one of each has EXACTLY the expected cardinality. This discriminates
    // the risky nullary siblings — if e.g. `ConditionalTrue`/`ConditionalFalse`
    // or `IndexedAccessObject`/`IndexedAccessIndex` ever aliased, the set would
    // shrink.
    let all = [
        // existing
        MergedContributor { ordinal: 0 },
        IntersectionArm { ordinal: 0 },
        Member { ordinal: 0 },
        MemberValue,
        TypeParamBound {
            ordinal: 0,
            position: TypeParamBoundPosition::Constraint,
        },
        // new authored-child positions
        FunctionParam { ordinal: 0 },
        FunctionReturn,
        ValueSignature { ordinal: 0 },
        MappedSource,
        MappedValue,
        MappedNameType,
        ConditionalCheck,
        ConditionalExtends,
        ConditionalTrue,
        ConditionalFalse,
        UnionArm { ordinal: 0 },
        IndexedAccessObject,
        IndexedAccessIndex,
        IndexSignatureKey,
        IndexSignatureValue,
        TupleElement { ordinal: 0 },
    ];
    assert_eq!(all.len(), 21, "5 existing + 16 new authored-child arms");
    let set: std::collections::HashSet<TypeBodyPathStep> = all.iter().copied().collect();
    assert_eq!(
        set.len(),
        all.len(),
        "every path-step arm must be a distinct content-free position (no arm aliases another)"
    );

    // The new arms compose inside a `TypeBodySlot` path: two slots differing
    // ONLY by a deep new-arm step are distinct identities (the arm carries
    // through the `Arc<[TypeBodyPathStep]>`).
    let path_slot = |last: TypeBodyPathStep| TypeBodySlot {
        anchor: anchor(),
        path: std::sync::Arc::from(vec![MemberValue, last].into_boxed_slice()),
    };
    assert_ne!(
        path_slot(FunctionParam { ordinal: 0 }),
        path_slot(FunctionReturn),
        "a new arm must discriminate a slot path"
    );
    assert_eq!(
        path_slot(ConditionalTrue),
        path_slot(ConditionalTrue),
        "the same path is the same slot identity"
    );
}

/// A local, structural mirror of `verter_session`'s `ExternalSymbolRef` (which
/// this crate cannot name) — proving `ExternalRouteRefFact` is field-1:1-usable
/// to reconstruct that session carrier at the B5 assembly boundary.
struct ExternalSymbolRefShape {
    local_name: String,
    source_specifier: String,
    imported_name: String,
    canonical_id: Option<std::sync::Arc<str>>,
    route: RouteDemand,
}

fn sample_external_route_ref() -> ExternalRouteRefFact {
    ExternalRouteRefFact {
        local_name: "Foo".to_string(),
        source_specifier: "./types".to_string(),
        imported_name: "FooProps".to_string(),
        canonical_id: Some(std::sync::Arc::from("/ws/types.ts")),
        route: RouteDemand::pick(["a", "b"]),
    }
}

#[test]
fn route_demand_facts_round_trip_and_reconstruct_external_symbol_ref_shape() {
    let ext = sample_external_route_ref();

    // Destructure EVERY field (a dropped/renamed field would fail to compile)
    // and rebuild the session-shaped value 1:1 — no field is lost in the fact.
    let ExternalRouteRefFact {
        local_name,
        source_specifier,
        imported_name,
        canonical_id,
        route,
    } = ext.clone();
    let reconstructed = ExternalSymbolRefShape {
        local_name,
        source_specifier,
        imported_name,
        canonical_id,
        route,
    };
    assert_eq!(reconstructed.local_name, "Foo");
    assert_eq!(reconstructed.source_specifier, "./types");
    assert_eq!(reconstructed.imported_name, "FooProps");
    assert_eq!(reconstructed.canonical_id.as_deref(), Some("/ws/types.ts"));
    assert_eq!(reconstructed.route, RouteDemand::pick(["a", "b"]));

    // `RouteDemand`'s four arms are each a distinct route kind. A bare name
    // string could not carry these — Pick/Omit/MemberPath over the SAME member
    // list stay distinct.
    let arms = [
        RouteDemand::Whole,
        RouteDemand::member_path(["x"]),
        RouteDemand::pick(["x"]),
        RouteDemand::omit(["x"]),
    ];
    let set: std::collections::HashSet<RouteDemand> = arms.iter().cloned().collect();
    assert_eq!(set.len(), 4, "the four route-demand arms are distinct");
    assert_ne!(RouteDemand::member_path(["x"]), RouteDemand::pick(["x"]));
    assert_ne!(RouteDemand::pick(["x"]), RouteDemand::omit(["x"]));

    // `RouteDependencyRefFact`: a local dep is a name + route demand; an
    // external dep wraps the full ref fact 1:1. The two are distinct dependency
    // kinds (never collapsed), and the local ROUTE participates in identity.
    let local = RouteDependencyRefFact::Local {
        name: "Bar".to_string(),
        route: RouteDemand::Whole,
    };
    let external = RouteDependencyRefFact::External(ext.clone());
    assert_ne!(local, external);
    assert_ne!(
        local,
        RouteDependencyRefFact::Local {
            name: "Bar".to_string(),
            route: RouteDemand::member_path(["x"]),
        },
        "a local dep's route demand is identity-participating"
    );
    match external {
        RouteDependencyRefFact::External(inner) => assert_eq!(inner, ext),
        RouteDependencyRefFact::Local { .. } => panic!("expected External"),
    }
}

#[test]
fn route_demand_pick_omit_eq_and_hash_agree_order_insensitively() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    // Pick/Omit are member SETS for cache identity: permuted member lists are
    // EQUAL by BOTH `Eq` and `Hash` (the normalized `RouteKeySet` inner). The
    // pre-collapse pair derived order-sensitive `Eq` while hashing sorted —
    // two values could hash equal yet compare unequal (the latent
    // cache-identity bug this collapse fixes; the `Eq` assertion here is the
    // FLIPPED pin).
    let pick_ab = RouteDemand::pick(["a", "b"]);
    let pick_ba = RouteDemand::pick(["b", "a"]);
    assert_eq!(
        pick_ab, pick_ba,
        "Pick equality is order-independent (normalized key set)"
    );
    assert_eq!(
        hash_of(&pick_ab),
        hash_of(&pick_ba),
        "Pick hashes its members as a normalized set — Eq and Hash agree"
    );

    let omit_ab = RouteDemand::omit(["a", "b"]);
    let omit_ba = RouteDemand::omit(["b", "a"]);
    assert_eq!(omit_ab, omit_ba, "Omit equality is order-independent");
    assert_eq!(
        hash_of(&omit_ab),
        hash_of(&omit_ba),
        "Omit hashes its members as a normalized set — Eq and Hash agree"
    );

    // Duplicate keys normalize away at construction.
    assert_eq!(RouteDemand::pick(["a", "a", "b"]), pick_ab);

    // MemberPath is a SEQUENCE (`Type['a']['b']` ≠ `Type['b']['a']`): both its
    // equality and its hash stay order-dependent.
    let path_ab = RouteDemand::member_path(["a", "b"]);
    let path_ba = RouteDemand::member_path(["b", "a"]);
    assert_ne!(path_ab, path_ba, "MemberPath equality is order-sensitive");
    assert_ne!(
        hash_of(&path_ab),
        hash_of(&path_ba),
        "MemberPath hashes segments in order"
    );

    // The discriminant hashes first: distinct arms over the same members hash
    // apart.
    assert_ne!(hash_of(&pick_ab), hash_of(&omit_ab));
    assert_ne!(hash_of(&path_ab), hash_of(&pick_ab));
    assert_ne!(hash_of(&RouteDemand::Whole), hash_of(&pick_ab));
}

#[test]
fn route_key_set_normalizes_at_serde_decode() {
    // A hand-crafted unnormalized payload re-normalizes at decode: the decoded
    // value equals the smart-constructed one by BOTH Eq and Hash.
    let decoded: RouteKeySet = serde_json::from_str(r#"["b","a","b"]"#).expect("decode");
    assert_eq!(decoded, RouteKeySet::new(["a", "b"]));
    assert_eq!(decoded.as_slice(), &["a".to_string(), "b".to_string()]);

    // Round-trip: normalized encode → identical decode.
    let encoded = serde_json::to_string(&RouteKeySet::new(["b", "a"])).expect("encode");
    assert_eq!(encoded, r#"["a","b"]"#);
}

#[test]
fn route_demand_whole_is_default() {
    assert_eq!(RouteDemand::default(), RouteDemand::Whole);
}

#[test]
fn route_demand_member_path_preserves_full_depth() {
    let path = RouteDemand::member_path(["variants", "color"]);
    match &path {
        RouteDemand::MemberPath(segments) => {
            assert_eq!(segments.len(), 2);
            assert_eq!(segments[0], "variants");
            assert_eq!(segments[1], "color");
        }
        _ => panic!("expected MemberPath"),
    }
}

#[test]
fn merge_identical_demands_returns_same() {
    let d = RouteDemand::member_path(["foo"]);
    assert_eq!(merge_route_demands(&d, &d), d);
}

#[test]
fn merge_member_paths_produces_pick() {
    let a = RouteDemand::member_path(["foo"]);
    let b = RouteDemand::member_path(["bar"]);
    let merged = merge_route_demands(&a, &b);
    assert_eq!(merged, RouteDemand::pick(["bar", "foo"]));
}

#[test]
fn merge_member_paths_with_common_prefix_keeps_prefix() {
    let a = RouteDemand::member_path(["variants", "color"]);
    let b = RouteDemand::member_path(["variants", "size"]);
    let merged = merge_route_demands(&a, &b);
    assert_eq!(merged, RouteDemand::member_path(["variants"]));
}

#[test]
fn merge_with_whole_always_returns_whole() {
    let a = RouteDemand::pick(["x"]);
    assert_eq!(
        merge_route_demands(&a, &RouteDemand::Whole),
        RouteDemand::Whole
    );
    assert_eq!(
        merge_route_demands(&RouteDemand::Whole, &a),
        RouteDemand::Whole
    );
}

#[test]
fn merge_pick_and_member_extends_pick() {
    let pick = RouteDemand::pick(["a", "b"]);
    let member = RouteDemand::member_path(["c"]);
    let merged = merge_route_demands(&pick, &member);
    assert_eq!(merged, RouteDemand::pick(["a", "b", "c"]));
}

#[test]
fn merge_omit_with_non_omitted_member_keeps_omit() {
    let omit = RouteDemand::omit(["a"]);
    let member = RouteDemand::member_path(["b"]);
    assert_eq!(merge_route_demands(&omit, &member), omit);
    assert_eq!(merge_route_demands(&member, &omit), omit);
    // An omitted member widens to Whole.
    let omitted_member = RouteDemand::member_path(["a"]);
    assert_eq!(
        merge_route_demands(&omit, &omitted_member),
        RouteDemand::Whole
    );
}

#[test]
fn fact_carriers_round_trip_through_serde() {
    // Persisted carriers (`Analyzed*` / `Expanded*` / `ResolvedLocalType`
    // narrowed positions) serialize the fact substrate. A DEEP value — a
    // four-source `SemanticTypeSource` nesting locators, span origins, and
    // closed facts — must round-trip byte-faithfully.
    let deep = SemanticTypeSource::Projected(ProjectedTypeFact::Member(ProjectedMemberFact {
        name: "value".to_string(),
        optional: true,
        readonly: false,
        is_method: false,
        visibility: MemberVisibility::Public,
        declared_in_macro_type_arg: true,
        declaration_origin: DeclarationOrigin::Declared(std::sync::Arc::from("/ws/types.ts")),
        ty: TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: std::sync::Arc::from("/ws/types.ts"),
                owner: crate::TopLevelOwnerId::ordinary_file(),
                symbol: std::sync::Arc::from("Props"),
                space: LocatorSymbolSpace::Type,
            },
            path: std::sync::Arc::from(vec![
                TypeBodyPathStep::Member { ordinal: 2 },
                TypeBodyPathStep::MemberValue,
            ]),
        },
        span_origin: MemberSpansOrigin::Authored {
            anchor: DeclContributorAnchor {
                contributor_index: 3,
                owner: crate::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 3,
            },
            member_path: std::sync::Arc::from(vec![2u32]),
        },
    }));
    let json = serde_json::to_string(&deep).expect("serialize");
    let back: SemanticTypeSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, deep, "deep fact round-trip must be identity");

    // The authored arm (a macro payload locator) round-trips too.
    let authored =
        SemanticTypeSource::Authored(AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: std::sync::Arc::from("/ws/App.vue"),
                owner: crate::TopLevelOwnerId::ordinary_file(),
                symbol: std::sync::Arc::from("default"),
                space: LocatorSymbolSpace::Value,
            },
            macro_index: 1,
            payload: MacroPayloadPosition::Field { field_index: 4 },
        }));
    let json = serde_json::to_string(&authored).expect("serialize");
    let back: SemanticTypeSource = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, authored);
    assert_ne!(back, deep, "distinct sources stay distinct after the trip");
}

#[test]
fn shallow_route_facts_preserve_route_in_identity_not_bare_strings() {
    let ext = sample_external_route_ref();
    let edge = MemberDependencyEdge {
        member: "m".to_string(),
        depends_on: std::sync::Arc::from(
            vec![RouteDependencyRefFact::Local {
                name: "n".to_string(),
                route: RouteDemand::Whole,
            }]
            .into_boxed_slice(),
        ),
    };
    let facts = ShallowRouteFacts {
        member_names: MemberNamesRoute::OpenKeyDomain,
        member_path_seed_edges: std::sync::Arc::from(
            vec![MemberPathSeedEdge {
                path: std::sync::Arc::from(vec!["a".to_string()].into_boxed_slice()),
                depends_on: MemberPathSeedTarget::ForwardBoundary(
                    RouteDependencyRefFact::External(ext.clone()),
                ),
            }]
            .into_boxed_slice(),
        ),
        member_dependency_edges: std::sync::Arc::from(vec![edge].into_boxed_slice()),
        whole_route_edges: std::sync::Arc::from(
            vec![WholeRouteEdgeFact::External {
                external_ref: ext.clone(),
                context: WholeRouteContextFact::Root,
            }]
            .into_boxed_slice(),
        ),
    };
    // The whole-route edges preserve the ROUTE in identity — a bare
    // `Arc<[String]>` field could not: changing only the route changes the
    // facts' identity.
    let mut facts2 = facts.clone();
    facts2.whole_route_edges = std::sync::Arc::from(
        vec![WholeRouteEdgeFact::External {
            external_ref: ExternalRouteRefFact {
                route: RouteDemand::Whole,
                ..ext.clone()
            },
            context: WholeRouteContextFact::Root,
        }]
        .into_boxed_slice(),
    );
    assert_ne!(
        facts, facts2,
        "whole_route_edges must carry the route in identity (not a bare name)"
    );
    assert_eq!(facts, facts.clone());

    // The per-edge CONTEXT participates in identity: the same 5-field external
    // ref at a transparent site (Root) vs a guarded site (LeafProperty) is a
    // DIFFERENT fact — `type B = Pick<Q,'a'>` vs `type B = { y: Pick<Q,'a'> }`
    // require opposite LeafProperty-follow behavior, so context can never be
    // dropped from the stored edge.
    let mut facts3 = facts.clone();
    facts3.whole_route_edges = std::sync::Arc::from(
        vec![WholeRouteEdgeFact::External {
            external_ref: ext.clone(),
            context: WholeRouteContextFact::LeafProperty,
        }]
        .into_boxed_slice(),
    );
    assert_ne!(
        facts, facts3,
        "the External arm's site context is identity-participating"
    );

    // Local edges carry name + route + context, all identity-participating.
    let local_edge =
        |route: RouteDemand, context: WholeRouteContextFact| WholeRouteEdgeFact::Local {
            name: "B".to_string(),
            route,
            context,
        };
    assert_ne!(
        local_edge(RouteDemand::Whole, WholeRouteContextFact::Root),
        local_edge(RouteDemand::Whole, WholeRouteContextFact::LeafProperty),
    );
    assert_ne!(
        local_edge(RouteDemand::Whole, WholeRouteContextFact::Root),
        local_edge(RouteDemand::pick(["a"]), WholeRouteContextFact::Root),
    );

    // Seed-edge targets discriminate terminal vs forward boundary: a terminal
    // dep list and a forward boundary over the SAME ref are different facts
    // (the union-terminal MISS depends on it).
    let terminal = MemberPathSeedEdge {
        path: std::sync::Arc::from(vec!["a".to_string()].into_boxed_slice()),
        depends_on: MemberPathSeedTarget::TerminalDeps(std::sync::Arc::from(
            vec![RouteDependencyRefFact::External(ext.clone())].into_boxed_slice(),
        )),
    };
    let forward = MemberPathSeedEdge {
        path: std::sync::Arc::from(vec!["a".to_string()].into_boxed_slice()),
        depends_on: MemberPathSeedTarget::ForwardBoundary(RouteDependencyRefFact::External(
            ext.clone(),
        )),
    };
    assert_ne!(terminal, forward);
}

#[test]
fn member_header_fact_carries_the_flags_from_eval_env_reconstructs() {
    // `decl_headers.rs::from_eval_env` builds a `MemberHeader { name, kind,
    // optional, readonly }` (+ visibility on the shared member surface). The fact
    // must carry each flag so a narrowed `TypeDeclInfo.direct_member_headers`
    // replaces the `merged_body().merged_member_names()` body walk.
    let base = MemberHeaderFact {
        name: "value".to_string(),
        is_method: false,
        optional: false,
        readonly: false,
        visibility: MemberVisibility::Public,
    };
    assert_ne!(
        base,
        MemberHeaderFact {
            name: "other".to_string(),
            ..base.clone()
        },
        "name must discriminate"
    );
    assert_ne!(
        base,
        MemberHeaderFact {
            is_method: true,
            ..base.clone()
        },
        "is_method must discriminate"
    );
    assert_ne!(
        base,
        MemberHeaderFact {
            optional: true,
            ..base.clone()
        },
        "optional must discriminate"
    );
    assert_ne!(
        base,
        MemberHeaderFact {
            readonly: true,
            ..base.clone()
        },
        "readonly must discriminate"
    );
    assert_ne!(
        base,
        MemberHeaderFact {
            visibility: MemberVisibility::Private,
            ..base.clone()
        },
        "visibility must discriminate"
    );
    assert_eq!(base, base.clone());

    // The enum-member-NAME superset fact carries the ordered member names the
    // `merged_enum_member_names` rail reads; order participates in identity.
    let names = EnumMemberNamesFact {
        names: std::sync::Arc::from(vec!["A".to_string(), "B".to_string()].into_boxed_slice()),
    };
    assert_eq!(names.names.len(), 2);
    assert_ne!(
        names,
        EnumMemberNamesFact {
            names: std::sync::Arc::from(vec!["B".to_string(), "A".to_string()].into_boxed_slice()),
        },
        "enum member NAME order participates in fact identity"
    );
}

#[test]
fn value_type_annotation_fact_holds_a_closed_inferred_annotation() {
    // An INFERRED / default-expression annotation has no authored `TSType` node,
    // so it cannot be a `TypeBodySlot` / `AuthoredBodyLocator`. The widened
    // `annotation` field carries it as a `SemanticTypeSource::Closed` closed fact
    // instead — never a fabricated authored locator.
    let inferred = ValueTypeAnnotationFact {
        typeof_alias_target: None,
        classification: ValueAnnotationClass::Direct,
        annotation: Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
            LeafTypeFact::Primitive(PrimitiveName::Number),
        ))),
    };
    // An authored TS annotation rides the SAME field under the `Authored` source
    // — proving the field widened to accept BOTH authored locators and closed
    // facts. A closed inferred source is a DISTINCT annotation from an authored
    // one.
    let authored = ValueTypeAnnotationFact {
        typeof_alias_target: None,
        classification: ValueAnnotationClass::Direct,
        annotation: Some(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
            slot(),
        ))),
    };
    assert_ne!(
        inferred, authored,
        "a closed inferred source must not alias an authored annotation source"
    );

    // `typeof_alias_target` still participates (the `typeof x[.y]` peel-target
    // replacement) and is orthogonal to the annotation source.
    let typeof_target = ValueTypeAnnotationFact {
        typeof_alias_target: Some(ValueDeclIdentityPart {
            canonical_id: std::sync::Arc::from("/ws/a.ts"),
            owner: crate::TopLevelOwnerId::ordinary_file(),
            symbol: std::sync::Arc::from("x"),
            member_path: std::sync::Arc::from(Vec::<String>::new().into_boxed_slice()),
        }),
        classification: ValueAnnotationClass::TypeOfAlias,
        annotation: None,
    };
    assert_ne!(typeof_target, inferred);
    assert_eq!(inferred, inferred.clone());
}
