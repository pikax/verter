use super::*;
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

fn prim(p: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(p)
}

fn name_ref(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new()),
    }
}

fn make_object(members: Vec<(&str, TypeExpr)>) -> TypeExpr {
    let properties: Vec<ObjectMember> = members
        .into_iter()
        .map(|(name, ty)| {
            ObjectMember::Property(ObjectProperty::synthetic_public(
                name.to_string(),
                ty,
                false,
                false,
            ))
        })
        .collect();
    TypeExpr::Object(Arc::new(ObjectExpr { properties }))
}

#[test]
fn primitive_is_stable_under_reuse() {
    let h1 = compute_semantic_hash(
        &prim(PrimitiveName::String),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    let h2 = compute_semantic_hash(
        &prim(PrimitiveName::String),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_eq!(h1.hash, h2.hash, "primitive must hash identically");
    assert!(!h1.budget_exceeded);
    assert_ne!(h1.visited_nodes, 0);
}

#[test]
fn constructor_type_and_function_with_same_signature_hash_differently() {
    // A constructor type `new (...) => R` and the function type `(...) => R`
    // are DISTINCT types — they must never collide in a content-addressed
    // cache key, even when they carry an identical `FunctionExpr`. The
    // walker emits a distinct discriminator (`0x73`) for `ConstructorType`
    // before the shared function body, so the two semantic hashes differ.
    // Pre-fix (no dedicated variant / no discriminator) the two would hash
    // identically; this test FAILS on that regression.
    let signature =
        verter_type_expr::FunctionExpr::synthetic(vec![], Some(Arc::new(name_ref("Foo"))), vec![]);
    let function = TypeExpr::Function(Arc::new(signature.clone()));
    let constructor = TypeExpr::ConstructorType(Arc::new(signature));

    let function_hash = compute_semantic_hash(&function, SymbolSpace::Type, &UnresolvedLens);
    let constructor_hash = compute_semantic_hash(&constructor, SymbolSpace::Type, &UnresolvedLens);

    assert_ne!(
        function_hash.hash, constructor_hash.hash,
        "a constructor type and a function type with the same signature must hash differently",
    );
    // Both must hash deterministically (re-hash matches).
    assert_eq!(
        constructor_hash.hash,
        compute_semantic_hash(&constructor, SymbolSpace::Type, &UnresolvedLens).hash,
        "constructor-type hash must be deterministic",
    );
}

#[test]
fn object_member_order_does_not_affect_hash() {
    // R16: alpha-normalised — member declaration order MUST NOT
    // change the semantic_hash.
    let obj_ab = make_object(vec![
        ("a", prim(PrimitiveName::Number)),
        ("b", prim(PrimitiveName::String)),
    ]);
    let obj_ba = make_object(vec![
        ("b", prim(PrimitiveName::String)),
        ("a", prim(PrimitiveName::Number)),
    ]);
    let h_ab = compute_semantic_hash(&obj_ab, SymbolSpace::Type, &UnresolvedLens);
    let h_ba = compute_semantic_hash(&obj_ba, SymbolSpace::Type, &UnresolvedLens);
    assert_eq!(
        h_ab.hash, h_ba.hash,
        "object member order MUST not change semantic_hash (alpha-normalised R16)"
    );
}

#[test]
fn property_value_edit_changes_hash() {
    // Discrimination: a real semantic edit DOES change the hash.
    let a = make_object(vec![("a", prim(PrimitiveName::String))]);
    let b = make_object(vec![("a", prim(PrimitiveName::Number))]);
    let h_a = compute_semantic_hash(&a, SymbolSpace::Type, &UnresolvedLens);
    let h_b = compute_semantic_hash(&b, SymbolSpace::Type, &UnresolvedLens);
    assert_ne!(
        h_a.hash, h_b.hash,
        "editing property body MUST change semantic_hash"
    );
}

#[test]
fn name_ref_emits_cross_decl_edge() {
    // R14 path-precision: a `Ref(Foo)` cross-decl reference
    // must be observable as a reference-shape edge, not by
    // inlining Foo's body.
    let ref_foo = name_ref("Foo");
    let ref_bar = name_ref("Bar");
    let h_foo = compute_semantic_hash(&ref_foo, SymbolSpace::Type, &UnresolvedLens);
    let h_bar = compute_semantic_hash(&ref_bar, SymbolSpace::Type, &UnresolvedLens);
    assert_ne!(
        h_foo.hash, h_bar.hash,
        "different ref names produce different hashes"
    );
}

#[test]
fn stack_safe_deep_chain_does_not_overflow() {
    // Build a left-leaning Union 200 deep — much deeper than
    // the default thread stack would tolerate via recursion.
    // The worklist hasher MUST terminate (it may set
    // `budget_exceeded` once depth ≥ 64).
    let mut node = prim(PrimitiveName::String);
    for _ in 0..200 {
        node = TypeExpr::Union(Arc::from(vec![node, prim(PrimitiveName::Number)]));
    }
    let result = compute_semantic_hash(&node, SymbolSpace::Type, &UnresolvedLens);
    assert!(
        result.budget_exceeded,
        "200-deep nesting MUST trigger budget_exceeded (limit = {})",
        MAX_HASH_DEPTH
    );
}

#[test]
fn shallow_object_does_not_exceed_budget() {
    // A small object that stays under 64 depth MUST NOT trip
    // budget_exceeded.
    let obj = make_object(vec![
        ("a", prim(PrimitiveName::String)),
        ("b", prim(PrimitiveName::Number)),
        ("c", prim(PrimitiveName::Boolean)),
    ]);
    let r = compute_semantic_hash(&obj, SymbolSpace::Type, &UnresolvedLens);
    assert!(
        !r.budget_exceeded,
        "shallow tree MUST stay under MAX_HASH_DEPTH"
    );
}

#[test]
fn cross_decl_lens_emits_distinct_shapes() {
    // Provide a lens that maps `Foo` → `LocalDecl(Foo, Type)`
    // and `Bar` → `ImportRef("./bar", "Bar", Type)`.
    struct MyLens;
    impl CrossDeclLens for MyLens {
        fn resolve(&self, name: &str, _space: SymbolSpace) -> Option<CrossDeclRef> {
            match name {
                "Foo" => Some(CrossDeclRef::LocalDecl {
                    name: Arc::from("Foo"),
                    space: SymbolSpace::Type,
                }),
                "Bar" => Some(CrossDeclRef::ImportRef {
                    specifier: Arc::from("./bar"),
                    binding: Arc::from("Bar"),
                    space: SymbolSpace::Type,
                }),
                _ => None,
            }
        }
    }
    let ref_foo = name_ref("Foo");
    let ref_bar = name_ref("Bar");
    let h_foo = compute_semantic_hash(&ref_foo, SymbolSpace::Type, &MyLens);
    let h_bar = compute_semantic_hash(&ref_bar, SymbolSpace::Type, &MyLens);
    // Different cross-decl shapes produce different hashes.
    assert_ne!(h_foo.hash, h_bar.hash);
    // Unresolved lens produces a third distinct hash for `Foo`.
    let h_foo_unresolved = compute_semantic_hash(&ref_foo, SymbolSpace::Type, &UnresolvedLens);
    assert_ne!(
        h_foo.hash, h_foo_unresolved.hash,
        "LocalDecl(Foo) and Unresolved(Foo) MUST differ"
    );
}

#[test]
fn member_presence_hash_independent_of_siblings() {
    // R28 two-fact model: `MemberPresence(Foo, "a")` MUST be
    // invariant under adding sibling `b`. Both formations of
    // `compute_member_presence_hash("Foo", "a", ...)` produce
    // the same fingerprint regardless of what else exists.
    let kind = MemberKind::Property {
        readonly: false,
        optional: false,
    };
    let h1 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    let h2 = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    assert_eq!(h1, h2, "presence hash MUST be deterministic");
    let h3 = compute_member_presence_hash("Foo", "b", kind, SymbolSpace::Type);
    assert_ne!(h1, h3, "different member name MUST hash distinctly");
    // Exporter salt distinguishes same-named members across
    // exporters in the same file.
    let h_foo_a = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    let h_bar_a = compute_member_presence_hash("Bar", "a", kind, SymbolSpace::Type);
    assert_ne!(
        h_foo_a, h_bar_a,
        "exporter qualifier salt MUST disambiguate same-named members"
    );
}

#[test]
fn member_presence_hash_changes_under_modifier_flip() {
    // R28: a property switching from required to optional MUST
    // change the presence hash (the consumer's view shifts).
    let required = MemberKind::Property {
        readonly: false,
        optional: false,
    };
    let optional = MemberKind::Property {
        readonly: false,
        optional: true,
    };
    let h_r = compute_member_presence_hash("Foo", "a", required, SymbolSpace::Type);
    let h_o = compute_member_presence_hash("Foo", "a", optional, SymbolSpace::Type);
    assert_ne!(h_r, h_o);
}

#[test]
fn member_shape_hash_invariant_under_member_reorder() {
    // R28 `MemberShape`: order-insensitive at top level. Sorted
    // by name.
    let kind = MemberKind::Property {
        readonly: false,
        optional: false,
    };
    let members_ab: Vec<(Arc<str>, MemberKind)> =
        vec![(Arc::from("a"), kind), (Arc::from("b"), kind)];
    let members_ba: Vec<(Arc<str>, MemberKind)> =
        vec![(Arc::from("b"), kind), (Arc::from("a"), kind)];
    let h_ab = compute_member_shape_hash("Foo", &members_ab, SymbolSpace::Type);
    let h_ba = compute_member_shape_hash("Foo", &members_ba, SymbolSpace::Type);
    assert_eq!(h_ab, h_ba, "member_shape MUST be order-insensitive");
}

#[test]
fn member_shape_hash_changes_when_member_added() {
    // R28: adding a member changes `MemberShape` but NOT each
    // existing `MemberPresence`.
    let kind = MemberKind::Property {
        readonly: false,
        optional: false,
    };
    let just_a: Vec<(Arc<str>, MemberKind)> = vec![(Arc::from("a"), kind)];
    let a_and_b: Vec<(Arc<str>, MemberKind)> = vec![(Arc::from("a"), kind), (Arc::from("b"), kind)];
    let h_a = compute_member_shape_hash("Foo", &just_a, SymbolSpace::Type);
    let h_ab = compute_member_shape_hash("Foo", &a_and_b, SymbolSpace::Type);
    assert_ne!(h_a, h_ab, "adding a member MUST change MemberShape");
    // And each MemberPresence is unchanged.
    let p_a_before = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    let p_a_after = compute_member_presence_hash("Foo", "a", kind, SymbolSpace::Type);
    assert_eq!(
        p_a_before, p_a_after,
        "MemberPresence(a) MUST be unchanged when sibling added"
    );
}

// ------------------------------------------------------------------
// Discrimination tests for the `TypeExpr::SyntheticSlotBinding`
// variant. The fact-hash walker must use a DISTINCT discriminator
// tag from `Ref` so that a synthetic carrier with
// `binding_name = "x"` does NOT collide with a workspace
// `TypeExpr::Ref { name: "x", type_arguments: [] }`.
// ------------------------------------------------------------------

fn synthetic_carrier(scope: &str, binding_name: &str, value_node: u64) -> TypeExpr {
    use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};
    TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
        scope_canonical_id: Arc::from(scope),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from(binding_name),
        value_node,
    })
}

#[test]
fn synthetic_carrier_fact_hash_differs_from_ref_with_same_name() {
    let carrier = synthetic_carrier("/abs/Foo.vue", "controls", 42);
    let plain_ref = name_ref("controls");

    let carrier_hash = compute_semantic_hash(&carrier, SymbolSpace::Type, &UnresolvedLens).hash;
    let ref_hash = compute_semantic_hash(&plain_ref, SymbolSpace::Type, &UnresolvedLens).hash;

    assert_ne!(
        carrier_hash, ref_hash,
        "synthetic carrier and workspace Ref with the same `name` MUST hash distinctly"
    );
}

#[test]
fn synthetic_carrier_fact_hash_value_node_discriminates() {
    // Same scope + binding_name, different value_node => distinct
    // hashes. Guards the rule that two same-binding-name carriers
    // in different slots of the same component are distinct
    // identities.
    let a = synthetic_carrier("/abs/Foo.vue", "controls", 1);
    let b = synthetic_carrier("/abs/Foo.vue", "controls", 2);

    let a_hash = compute_semantic_hash(&a, SymbolSpace::Type, &UnresolvedLens).hash;
    let b_hash = compute_semantic_hash(&b, SymbolSpace::Type, &UnresolvedLens).hash;

    assert_ne!(
        a_hash, b_hash,
        "synthetic carriers differing only in value_node MUST hash distinctly"
    );
}

fn object_with_property_visibility(vis: verter_type_expr::MemberVisibility) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::with_visibility(
            "x".to_string(),
            prim(PrimitiveName::Number),
            false,
            false,
            vis,
            verter_type_expr::MemberSpans::default(),
        ))],
    }))
}

fn object_with_method_visibility(vis: verter_type_expr::MemberVisibility) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Method(MethodSignature::with_visibility(
            "m".to_string(),
            FunctionExpr::synthetic(vec![], None, vec![]),
            false,
            vis,
            verter_type_expr::MemberSpans::default(),
        ))],
    }))
}

/// Two object types identical except a PROPERTY member's visibility must
/// produce DISTINCT fact hashes — `TypeExpr` node identity already
/// distinguishes them, so omitting visibility from the fact hasher would be
/// a cache-correctness gap (public/protected/private collide).
///
/// Discrimination: against the tree where `write_property` omits visibility,
/// all three hashes are EQUAL and every `assert_ne!` FAILS.
#[test]
fn property_visibility_discriminates_fact_hash() {
    use verter_type_expr::MemberVisibility::{Private, Protected, Public};
    let pub_h = compute_semantic_hash(
        &object_with_property_visibility(Public),
        SymbolSpace::Type,
        &UnresolvedLens,
    )
    .hash;
    let prot_h = compute_semantic_hash(
        &object_with_property_visibility(Protected),
        SymbolSpace::Type,
        &UnresolvedLens,
    )
    .hash;
    let priv_h = compute_semantic_hash(
        &object_with_property_visibility(Private),
        SymbolSpace::Type,
        &UnresolvedLens,
    )
    .hash;
    assert_ne!(pub_h, prot_h, "public vs protected property must differ");
    assert_ne!(pub_h, priv_h, "public vs private property must differ");
    assert_ne!(prot_h, priv_h, "protected vs private property must differ");
}

/// Two object types identical except a METHOD member's visibility must
/// produce DISTINCT fact hashes (same rationale as the property case).
#[test]
fn method_visibility_discriminates_fact_hash() {
    use verter_type_expr::MemberVisibility::{Private, Protected, Public};
    let pub_h = compute_semantic_hash(
        &object_with_method_visibility(Public),
        SymbolSpace::Type,
        &UnresolvedLens,
    )
    .hash;
    let prot_h = compute_semantic_hash(
        &object_with_method_visibility(Protected),
        SymbolSpace::Type,
        &UnresolvedLens,
    )
    .hash;
    let priv_h = compute_semantic_hash(
        &object_with_method_visibility(Private),
        SymbolSpace::Type,
        &UnresolvedLens,
    )
    .hash;
    assert_ne!(pub_h, prot_h, "public vs protected method must differ");
    assert_ne!(pub_h, priv_h, "public vs private method must differ");
    assert_ne!(prot_h, priv_h, "protected vs private method must differ");
}

/// Two all-public objects built via DIFFERENT public constructors
/// (`synthetic` vs explicit `Public`) hash identically — the marker is
/// only-for-non-public, so public fact identity is unchanged.
#[test]
fn all_public_object_fact_hash_is_marker_free() {
    use verter_type_expr::MemberVisibility::Public;
    let via_synthetic = make_object(vec![("x", prim(PrimitiveName::Number))]);
    let via_explicit_public = object_with_property_visibility(Public);
    // The two only differ in how visibility was constructed (both Public)
    // and the property name/type match, so their fact hashes are equal.
    let a = compute_semantic_hash(&via_synthetic, SymbolSpace::Type, &UnresolvedLens).hash;
    let b = compute_semantic_hash(&via_explicit_public, SymbolSpace::Type, &UnresolvedLens).hash;
    assert_eq!(
        a, b,
        "an all-public object's fact hash must not depend on how Public was constructed",
    );
}

// ------------------------------------------------------------------
// Body-fingerprint parity: the borrowed-transient `type_body_fingerprint`
// / `value_body_fingerprint` producers emit byte-identical output to
// the legacy `compute_semantic_hash` over the hand-built folded body
// view — so computing the hash at lazy decl-body lowering time (from the
// transient lowered bodies, before locator narrowing) changes ZERO cache
// generations. Every parity oracle below is GOLDEN: the expected value is
// reconstructed INDEPENDENTLY of the producer by hand-building the exact
// legacy `TypeExpr` view and hashing it through the unchanged
// `compute_semantic_hash` grammar. Each surface also discriminates a real
// body edit.
// ------------------------------------------------------------------

use verter_type_expr::facts::{
    EnumPrimitiveDomain, EnumScalar, FunctionParamFact, NarrowTypeParam,
};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, TypeBodySlot};
use verter_type_expr::span_origins::{DeclContributorAnchor, FunctionSpansOrigin};

fn num(n: f64) -> TypeExpr {
    TypeExpr::Literal(LiteralValue::Number(n))
}

fn folded_num(s: &str) -> EnumMemberValue {
    EnumMemberValue::Folded(EnumScalar::Number(s.to_string()))
}

fn folded_str(s: &str) -> EnumMemberValue {
    EnumMemberValue::Folded(EnumScalar::String(s.to_string()))
}

#[test]
fn type_body_fingerprint_single_matches_legacy_body_hash() {
    // A `Single` transient body hashes byte-identically to hashing the
    // borrowed `TypeExpr` directly (the legacy folded view for a single
    // declaration IS the body).
    let obj = make_object(vec![
        ("a", prim(PrimitiveName::Number)),
        ("b", prim(PrimitiveName::String)),
    ]);
    let via_producer = type_body_fingerprint(
        TransientTypeBody::Single(&obj),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    let legacy = compute_semantic_hash(&obj, SymbolSpace::Type, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy,
        "type_body_fingerprint(Single) MUST equal compute_semantic_hash over the body"
    );
    // Discrimination: a property-value edit MUST move the producer hash.
    let edited_obj = make_object(vec![
        ("a", prim(PrimitiveName::String)),
        ("b", prim(PrimitiveName::String)),
    ]);
    let edited_h = type_body_fingerprint(
        TransientTypeBody::Single(&edited_obj),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_ne!(
        via_producer.hash, edited_h.hash,
        "editing a member body MUST change the type body fingerprint"
    );
}

#[test]
fn type_body_fingerprint_merged_matches_legacy_folded_object_view() {
    // A `Merged` input folds every contributor's DIRECT object members into
    // one object view — descending `Intersection` / `Parenthesized` arms and
    // skipping heritage `Ref` arms — and the producer hashes exactly that
    // folded view. The golden is the hand-built legacy fold: one `Object`
    // carrying c1's members then c2's own members, in contributor order.
    let c1 = make_object(vec![("a", prim(PrimitiveName::Number))]);
    // A heritage-carrying contributor: `interface B extends Base { b: string }`
    // lowers to Intersection([Ref Base, <own Object>]); the legacy fold
    // descends the intersection (and any parenthesization) and collects ONLY
    // the own object members — the heritage Ref arm contributes nothing.
    let c2 = TypeExpr::Intersection(Arc::from(vec![
        name_ref("Base"),
        TypeExpr::Parenthesized(Arc::new(make_object(vec![(
            "b",
            prim(PrimitiveName::String),
        )]))),
    ]));
    let contributors = vec![c1.clone(), c2.clone()];
    let via_producer = type_body_fingerprint(
        TransientTypeBody::Merged(&contributors),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    let legacy_view = make_object(vec![
        ("a", prim(PrimitiveName::Number)),
        ("b", prim(PrimitiveName::String)),
    ]);
    let legacy = compute_semantic_hash(&legacy_view, SymbolSpace::Type, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy,
        "type_body_fingerprint(Merged) MUST equal compute_semantic_hash over the hand-built \
         legacy folded view (direct members only, heritage Ref arms skipped)"
    );
    // Discrimination 1: the fold of two contributors differs from hashing
    // only the first contributor — so a dropped/added contributor moves
    // the hash (the fold is real, not last-wins).
    let only_c1 = compute_semantic_hash(&c1, SymbolSpace::Type, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, only_c1.hash,
        "merged fold MUST include every contributor's members, not just the first"
    );
    // Discrimination 2: a member-value edit inside the second contributor
    // moves the fingerprint.
    let c2_edited = TypeExpr::Intersection(Arc::from(vec![
        name_ref("Base"),
        TypeExpr::Parenthesized(Arc::new(make_object(vec![(
            "b",
            prim(PrimitiveName::Number),
        )]))),
    ]));
    let edited_contributors = vec![c1, c2_edited];
    let edited_h = type_body_fingerprint(
        TransientTypeBody::Merged(&edited_contributors),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_ne!(
        via_producer.hash, edited_h.hash,
        "editing a contributor's member body MUST change the merged fingerprint"
    );
}

#[test]
fn type_body_fingerprint_enum_union_matches_legacy_projected_union() {
    // The enum TYPE-space union: every member's projected scalar becomes its
    // legacy projected `TypeExpr` arm — a folded numeric scalar is the exact
    // number literal, a folded string scalar the string literal, and a
    // deferred member's degraded domain the legacy primitive arm
    // (`NumberOrString` is the nested `number | string` union). The golden is
    // the hand-built legacy `TypeExpr::union` over those arms.
    let scalars = vec![
        EnumScalar::Number("0".to_string()),
        EnumScalar::String("go".to_string()),
        EnumScalar::Primitive(EnumPrimitiveDomain::Number),
        EnumScalar::Primitive(EnumPrimitiveDomain::String),
        EnumScalar::Primitive(EnumPrimitiveDomain::NumberOrString),
        EnumScalar::Primitive(EnumPrimitiveDomain::Unknown),
    ];
    let via_producer = type_body_fingerprint(
        TransientTypeBody::EnumUnion(&scalars),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    let legacy_union = TypeExpr::union(vec![
        num(0.0),
        TypeExpr::string_literal("go"),
        prim(PrimitiveName::Number),
        prim(PrimitiveName::String),
        TypeExpr::union(vec![
            prim(PrimitiveName::Number),
            prim(PrimitiveName::String),
        ]),
        prim(PrimitiveName::Unknown),
    ]);
    let legacy = compute_semantic_hash(&legacy_union, SymbolSpace::Type, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy,
        "type_body_fingerprint(EnumUnion) MUST equal compute_semantic_hash over the hand-built \
         legacy projected-type union (folded literals + degraded primitive-domain arms)"
    );
    // Discrimination 1: a folded arm's numeric value edit moves the hash.
    let mut edited = scalars.clone();
    edited[0] = EnumScalar::Number("3".to_string());
    let edited_h = type_body_fingerprint(
        TransientTypeBody::EnumUnion(&edited),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_ne!(
        via_producer.hash, edited_h.hash,
        "editing a folded enum member's value MUST change the enum-union fingerprint"
    );
    // Discrimination 2: dropping a degraded-domain arm moves the hash (the
    // union carries the deferred members' arms, not just the folded subset).
    let without_number_or_string: Vec<EnumScalar> = scalars
        .iter()
        .filter(|s| {
            !matches!(
                s,
                EnumScalar::Primitive(EnumPrimitiveDomain::NumberOrString)
            )
        })
        .cloned()
        .collect();
    let dropped_h = type_body_fingerprint(
        TransientTypeBody::EnumUnion(&without_number_or_string),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_ne!(
        via_producer.hash, dropped_h.hash,
        "dropping a deferred member's degraded arm MUST change the enum-union fingerprint"
    );
}

#[test]
fn type_body_fingerprint_enum_union_number_round_trips_exact_f64_bits() {
    // CRITICAL byte-parity: `EnumScalar::Number` stores the canonical `f64`
    // display string, and the fact grammar emits number literals as
    // `f64::to_bits().to_le_bytes()`. The producer MUST parse the scalar back
    // to the exact bits — for values like 1.5 / 100 / 0 a raw string-byte
    // emission produces different fact bytes than the legacy stored literal.
    for (repr, value) in [("1.5", 1.5_f64), ("100", 100.0_f64), ("0", 0.0_f64)] {
        let scalars = vec![EnumScalar::Number(repr.to_string())];
        let via_producer = type_body_fingerprint(
            TransientTypeBody::EnumUnion(&scalars),
            SymbolSpace::Type,
            &UnresolvedLens,
        );
        // Single-arm legacy union unwraps to the bare literal — same as the
        // producer's `TypeExpr::union` build.
        let legacy = compute_semantic_hash(&num(value), SymbolSpace::Type, &UnresolvedLens);
        assert_eq!(
            via_producer, legacy,
            "EnumUnion([Number({repr:?})]) MUST hash as the exact f64 literal {value}"
        );
        // The naive raw-string emission trap: the same digits as a STRING
        // literal fact-hash differently (0x10 0x00 + utf8 vs 0x10 0x01 + bits).
        let as_string_literal = compute_semantic_hash(
            &TypeExpr::string_literal(repr),
            SymbolSpace::Type,
            &UnresolvedLens,
        );
        assert_ne!(
            via_producer.hash, as_string_literal.hash,
            "a numeric scalar MUST NOT fingerprint as its raw digit string"
        );
    }
}

#[test]
fn type_body_fingerprint_enum_union_empty_and_single_match_legacy_unwrap() {
    // Legacy built the enum type body with `TypeExpr::union(arms)`: an empty
    // arm set is `never`, a single arm unwraps to the bare arm. The producer
    // reproduces both edge shapes byte-identically.
    let empty: Vec<EnumScalar> = Vec::new();
    let empty_h = type_body_fingerprint(
        TransientTypeBody::EnumUnion(&empty),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    let never = compute_semantic_hash(
        &prim(PrimitiveName::Never),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_eq!(
        empty_h, never,
        "an empty enum arm set MUST fingerprint as `never` (legacy empty-union unwrap)"
    );

    let single = vec![EnumScalar::String("only".to_string())];
    let single_h = type_body_fingerprint(
        TransientTypeBody::EnumUnion(&single),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    let bare = compute_semantic_hash(
        &TypeExpr::string_literal("only"),
        SymbolSpace::Type,
        &UnresolvedLens,
    );
    assert_eq!(
        single_h, bare,
        "a single-arm enum union MUST fingerprint as the bare arm (legacy single-union unwrap)"
    );
}

#[test]
fn value_body_fingerprint_annotation_matches_legacy_annotation_hash() {
    // A value decl with a type annotation hashes byte-identically to
    // hashing the annotation directly.
    let annotation = prim(PrimitiveName::Boolean);
    let input =
        ValueBodyFingerprintInput::new(Some(&annotation), &[], ValueDeclKind::Const, None, None);
    let via_producer = value_body_fingerprint(&input, SymbolSpace::Value, &UnresolvedLens);
    let legacy = compute_semantic_hash(&annotation, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer.hash, legacy.hash,
        "value_body_fingerprint(annotation) MUST equal compute_semantic_hash over the annotation"
    );
}

#[test]
fn value_body_fingerprint_enum_folds_members_and_discriminates() {
    // An enum with foldable members folds into an object (member NAME →
    // value-literal); the producer hashes exactly that object, minting each
    // literal from the stored scalar. Numeric scalars MUST round-trip to the
    // exact f64 bits (1.5 / 100 / 0 are the values where a raw string-byte
    // emission diverges from the legacy stored `number_literal` bytes), and a
    // string scalar folds to the string literal.
    let members_v0: Vec<(String, EnumMemberValue)> = vec![
        ("Red".to_string(), folded_num("0")),
        ("Green".to_string(), folded_num("1")),
        ("Half".to_string(), folded_num("1.5")),
        ("Hundred".to_string(), folded_num("100")),
        ("Name".to_string(), folded_str("red")),
    ];
    let input_v0 =
        ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Enum, None, Some(&members_v0));
    let h_v0 = value_body_fingerprint(&input_v0, SymbolSpace::Value, &UnresolvedLens);

    // Byte-parity against the hand-assembled legacy folded object view (each
    // member's stored literal, readonly + non-optional + public).
    let expected_obj = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Red".to_string(),
                num(0.0),
                false,
                true,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Green".to_string(),
                num(1.0),
                false,
                true,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Half".to_string(),
                num(1.5),
                false,
                true,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Hundred".to_string(),
                num(100.0),
                false,
                true,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Name".to_string(),
                TypeExpr::string_literal("red"),
                false,
                true,
            )),
        ],
    }));
    let legacy = compute_semantic_hash(&expected_obj, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        h_v0, legacy,
        "enum value_body_fingerprint MUST equal compute_semantic_hash over the folded object"
    );

    // Discrimination: flipping a member's folded value moves the hash.
    let mut members_v1 = members_v0.clone();
    members_v1[1] = ("Green".to_string(), folded_num("2"));
    let input_v1 =
        ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Enum, None, Some(&members_v1));
    let h_v1 = value_body_fingerprint(&input_v1, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        h_v0.hash, h_v1.hash,
        "editing an enum member's folded value MUST change the value body fingerprint"
    );
}

#[test]
fn value_body_fingerprint_fallback_matches_legacy_unknown_shape() {
    // No annotation, no signatures, non-enum kind → the legacy synthesised
    // `Unknown { raw: "<kind>::<object_shape>" }` carrier. The producer
    // reproduces that exact fallback byte-for-byte.
    let input = ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Const, None, None);
    let via_producer = value_body_fingerprint(&input, SymbolSpace::Value, &UnresolvedLens);
    let expected = TypeExpr::Unknown {
        raw: format!("{:?}::{:?}", ValueDeclKind::Const, None::<&ObjectExpr>),
    };
    let legacy = compute_semantic_hash(&expected, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer.hash, legacy.hash,
        "value_body_fingerprint fallback MUST equal the legacy synthesised Unknown carrier hash"
    );
}

/// Build a minimal signature FACT for the value-body fingerprint tests:
/// no type parameters, no parameters, an optional authored-return body slot.
fn signature_fact(has_authored_return: bool) -> FunctionSignature {
    FunctionSignature {
        type_parameters: Arc::from(Vec::<NarrowTypeParam>::new().into_boxed_slice()),
        parameters: Arc::from(Vec::<FunctionParamFact>::new().into_boxed_slice()),
        return_ty: has_authored_return.then(|| TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/ws/a.ts"),
                symbol: Arc::from("f"),
                space: LocatorSymbolSpace::Value,
            },
            path: Arc::from(
                Vec::<verter_type_expr::locators::TypeBodyPathStep>::new().into_boxed_slice(),
            ),
        }),
        has_implementation_body: false,
        spans_origin: FunctionSpansOrigin::AliasBody {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
            },
        },
    }
}

#[test]
fn value_body_fingerprint_signatures_matches_legacy_unknown_carrier_and_discriminates() {
    // No annotation, non-enum kind, NON-EMPTY signature set → the legacy
    // synthesised `Unknown { raw: format!("{signatures:?}") }` debug carrier.
    // The expected fingerprint is reconstructed INDEPENDENTLY of the producer:
    // hand-build the exact legacy `Unknown` node and hash it through
    // `compute_semantic_hash`, so a byte-grammar divergence in the producer's
    // signatures branch fails this assertion.
    let signatures = vec![signature_fact(true)];
    let input =
        ValueBodyFingerprintInput::new(None, &signatures, ValueDeclKind::Function, None, None);
    let via_producer = value_body_fingerprint(&input, SymbolSpace::Value, &UnresolvedLens);

    let expected = TypeExpr::Unknown {
        raw: format!("{:?}", signatures.as_slice()),
    };
    let legacy = compute_semantic_hash(&expected, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy,
        "value_body_fingerprint over a signature-bearing decl MUST equal the legacy \
         signature-debug Unknown carrier hash"
    );

    // Discrimination: a signature-fact edit (dropping the authored return
    // slot) moves the fingerprint.
    let edited_signatures = vec![signature_fact(false)];
    let edited_input = ValueBodyFingerprintInput::new(
        None,
        &edited_signatures,
        ValueDeclKind::Function,
        None,
        None,
    );
    let edited = value_body_fingerprint(&edited_input, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, edited.hash,
        "editing a signature fact MUST change the value body fingerprint"
    );
}

#[test]
fn value_body_fingerprint_object_shape_fallback_matches_legacy_and_discriminates() {
    // No annotation, no signatures, non-enum kind, PRESENT object shape → the
    // legacy `Unknown { raw: "<kind>::<object_shape>" }` carrier with the
    // shape's debug body embedded. Reconstructed independently of the producer
    // (hand-built `Unknown` node through `compute_semantic_hash`), so a
    // divergence in the producer's kind/object-shape fallback bytes fails here.
    let shape = ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "x".to_string(),
            prim(PrimitiveName::Number),
            false,
            false,
        ))],
    };
    let input = ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Const, Some(&shape), None);
    let via_producer = value_body_fingerprint(&input, SymbolSpace::Value, &UnresolvedLens);

    let expected = TypeExpr::Unknown {
        raw: format!("{:?}::{:?}", ValueDeclKind::Const, Some(&shape)),
    };
    let legacy = compute_semantic_hash(&expected, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy,
        "value_body_fingerprint over a shaped shapeless-kind decl MUST equal the legacy \
         kind/object-shape Unknown carrier hash"
    );

    // Discrimination 1: an object-shape member edit moves the fingerprint.
    let edited_shape = ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "x".to_string(),
            prim(PrimitiveName::String),
            false,
            false,
        ))],
    };
    let edited_input =
        ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Const, Some(&edited_shape), None);
    let edited = value_body_fingerprint(&edited_input, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, edited.hash,
        "editing the object shape MUST change the value body fingerprint"
    );

    // Discrimination 2: dropping the shape entirely moves the fingerprint
    // (Some(shape) and None must not collide).
    let shapeless = ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Const, None, None);
    let shapeless_h = value_body_fingerprint(&shapeless, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, shapeless_h.hash,
        "a present object shape MUST fingerprint differently from the shapeless fallback"
    );
}

#[test]
fn value_body_fingerprint_mixed_enum_folds_only_foldable_members_and_discriminates() {
    // A MIXED enum: foldable members + a deferred (unfoldable) member. The
    // legacy grammar filters to the foldable members BEFORE building the object
    // carrier — a deferred member is projected out of the fingerprint entirely.
    // The expected object is hand-assembled independently of the producer
    // (foldable-only properties, source order, readonly + non-optional +
    // public), so a filtering or property-emission divergence fails here.
    let members: Vec<(String, EnumMemberValue)> = vec![
        (
            "Computed".to_string(),
            EnumMemberValue::Deferred(EnumPrimitiveDomain::Number),
        ),
        ("Red".to_string(), folded_num("0")),
        ("Green".to_string(), folded_num("1")),
    ];
    let input =
        ValueBodyFingerprintInput::new(None, &[], ValueDeclKind::Enum, None, Some(&members));
    let via_producer = value_body_fingerprint(&input, SymbolSpace::Value, &UnresolvedLens);

    let expected_obj = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Red".to_string(),
                num(0.0),
                false,
                true,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public(
                "Green".to_string(),
                num(1.0),
                false,
                true,
            )),
        ],
    }));
    let legacy = compute_semantic_hash(&expected_obj, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy,
        "a mixed enum's value_body_fingerprint MUST equal the hash of the hand-assembled \
         foldable-only object (the deferred member projected out)"
    );

    // Negative: a deferred member's degraded DOMAIN is not a value edit — the
    // fingerprint ignores it.
    let members_deferred_edit: Vec<(String, EnumMemberValue)> = vec![
        (
            "Computed".to_string(),
            EnumMemberValue::Deferred(EnumPrimitiveDomain::String),
        ),
        ("Red".to_string(), folded_num("0")),
        ("Green".to_string(), folded_num("1")),
    ];
    let input_deferred_edit = ValueBodyFingerprintInput::new(
        None,
        &[],
        ValueDeclKind::Enum,
        None,
        Some(&members_deferred_edit),
    );
    let deferred_edit_h =
        value_body_fingerprint(&input_deferred_edit, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer.hash, deferred_edit_h.hash,
        "editing a deferred member's degraded domain MUST NOT change the value body fingerprint"
    );

    // Discrimination: the deferred member BECOMING foldable enters the fold and
    // moves the fingerprint (the filter discriminates on foldability).
    let members_folded_third: Vec<(String, EnumMemberValue)> = vec![
        ("Computed".to_string(), folded_num("5")),
        ("Red".to_string(), folded_num("0")),
        ("Green".to_string(), folded_num("1")),
    ];
    let input_folded_third = ValueBodyFingerprintInput::new(
        None,
        &[],
        ValueDeclKind::Enum,
        None,
        Some(&members_folded_third),
    );
    let folded_third_h =
        value_body_fingerprint(&input_folded_third, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, folded_third_h.hash,
        "a deferred member becoming foldable MUST change the value body fingerprint"
    );
}

#[test]
fn value_body_fingerprint_enum_members_take_precedence_over_annotation() {
    // An enum decl carrying BOTH a member inventory and a type annotation
    // fingerprints as the folded member object — the enum-members branch wins
    // over the annotation walk. Parity is asserted against the independently
    // hand-assembled folded object, and precedence is proven both ways: an
    // annotation edit does not move the fingerprint, a member edit does.
    let members: Vec<(String, EnumMemberValue)> = vec![("Red".to_string(), folded_num("0"))];
    let annotation = prim(PrimitiveName::Boolean);
    let input = ValueBodyFingerprintInput::new(
        Some(&annotation),
        &[],
        ValueDeclKind::Enum,
        None,
        Some(&members),
    );
    let via_producer = value_body_fingerprint(&input, SymbolSpace::Value, &UnresolvedLens);

    let expected_obj = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "Red".to_string(),
            num(0.0),
            false,
            true,
        ))],
    }));
    let legacy_folded = compute_semantic_hash(&expected_obj, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer, legacy_folded,
        "an enum with members AND an annotation MUST fingerprint as the folded member object"
    );
    let annotation_walk = compute_semantic_hash(&annotation, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, annotation_walk.hash,
        "the enum fold MUST NOT collapse to the annotation walk"
    );

    // Precedence negative: editing the (shadowed) annotation does not move the
    // fingerprint.
    let other_annotation = prim(PrimitiveName::String);
    let input_annotation_edit = ValueBodyFingerprintInput::new(
        Some(&other_annotation),
        &[],
        ValueDeclKind::Enum,
        None,
        Some(&members),
    );
    let annotation_edit_h =
        value_body_fingerprint(&input_annotation_edit, SymbolSpace::Value, &UnresolvedLens);
    assert_eq!(
        via_producer.hash, annotation_edit_h.hash,
        "editing the annotation of an enum with a member inventory MUST NOT change the fingerprint"
    );

    // Discrimination: a member value edit moves the fingerprint.
    let members_edit: Vec<(String, EnumMemberValue)> = vec![("Red".to_string(), folded_num("1"))];
    let input_member_edit = ValueBodyFingerprintInput::new(
        Some(&annotation),
        &[],
        ValueDeclKind::Enum,
        None,
        Some(&members_edit),
    );
    let member_edit_h =
        value_body_fingerprint(&input_member_edit, SymbolSpace::Value, &UnresolvedLens);
    assert_ne!(
        via_producer.hash, member_edit_h.hash,
        "editing a folded member's value MUST change the value body fingerprint"
    );
}
