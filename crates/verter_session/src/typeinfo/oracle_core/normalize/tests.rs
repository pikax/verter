//! Discriminating guards for the confluent oracle normalizer (§Q2 / §4).
//!
//! Each guard either proves a soundness property of the normalizer (idempotence,
//! confluence, termination) or proves a specific rewrite is BOTH
//! semantics-preserving AND discriminating (it does not over-collapse a real
//! difference). The confluence guard is the central one: it feeds two
//! differently-spelled but equal inputs through the SAME pipeline and asserts
//! byte-equality, AND asserts unequal inputs still diverge.

use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, LiteralValue, MethodSignature, ObjectExpr,
    ObjectMember, ObjectProperty, PrimitiveName, TypeExpr, TypeParam,
};

use super::{normalize, normalized_canonical_json, NormalizeReject, ProjectionModeKind};

const M: ProjectionModeKind = ProjectionModeKind::Shallow;

// -- tiny TypeExpr builders ------------------------------------------------

fn prim(p: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(p)
}
fn slit(s: &str) -> TypeExpr {
    TypeExpr::string_literal(s)
}
fn nlit(n: f64) -> TypeExpr {
    TypeExpr::number_literal(n)
}
fn blit(b: bool) -> TypeExpr {
    TypeExpr::boolean_literal(b)
}
fn union(arms: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Union(Arc::from(arms))
}
fn intersection(arms: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Intersection(Arc::from(arms))
}
fn obj(members: Vec<ObjectMember>) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }))
}
fn prop(name: &str, ty: TypeExpr, optional: bool, readonly: bool) -> ObjectMember {
    ObjectMember::Property(ObjectProperty::synthetic_public(
        name.to_string(),
        ty,
        optional,
        readonly,
    ))
}
fn idx(key_name: &str, value: TypeExpr) -> ObjectMember {
    ObjectMember::IndexSignature(IndexSignature::synthetic(
        key_name.to_string(),
        prim(PrimitiveName::String),
        value,
        false,
    ))
}
fn type_ref(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new()),
    }
}

fn norm(x: &TypeExpr) -> String {
    normalized_canonical_json(x, M).expect("admissible input must normalize")
}

// =========================================================================
// oracle_normalization_is_idempotent
// =========================================================================

#[test]
fn oracle_normalization_is_idempotent() {
    let inputs = [
        union(vec![
            prim(PrimitiveName::Number),
            prim(PrimitiveName::String),
        ]),
        union(vec![blit(true), blit(false), prim(PrimitiveName::Number)]),
        obj(vec![
            prop("b", prim(PrimitiveName::Number), false, false),
            prop("a", prim(PrimitiveName::String), true, false),
        ]),
        intersection(vec![
            obj(vec![prop("x", prim(PrimitiveName::Number), false, false)]),
            prim(PrimitiveName::Unknown),
        ]),
    ];
    for input in inputs {
        let once = normalize(&input, M).expect("normalize");
        let twice = normalize(&once, M).expect("normalize again");
        assert_eq!(
            super::canonical_json_string(&once.to_json_value()),
            super::canonical_json_string(&twice.to_json_value()),
            "normalize(normalize(x)) must equal normalize(x)"
        );
    }
}

// =========================================================================
// oracle_normalization_is_confluent  (the central soundness obligation)
// =========================================================================

#[test]
fn oracle_normalization_is_confluent() {
    // Each pair is two DIFFERENTLY-SPELLED but semantically-EQUAL inputs that
    // MUST normalize byte-equal — including the rule-composition cases a single
    // pass would miss.
    let equal_pairs: Vec<(TypeExpr, TypeExpr)> = vec![
        // ≥3-arm boolean case the exact-two-arm rule missed.
        (
            union(vec![blit(true), blit(false), prim(PrimitiveName::Number)]),
            union(vec![
                prim(PrimitiveName::Boolean),
                prim(PrimitiveName::Number),
            ]),
        ),
        // step-5 boolean → boolean|boolean which the fixpoint re-dedups.
        (
            union(vec![blit(true), blit(false), prim(PrimitiveName::Boolean)]),
            prim(PrimitiveName::Boolean),
        ),
        // arm-order-permuted.
        (
            union(vec![prim(PrimitiveName::Boolean), blit(true), blit(false)]),
            prim(PrimitiveName::Boolean),
        ),
        // two literals both subsumed by a co-present `string`.
        (
            union(vec![prim(PrimitiveName::String), slit("a"), slit("b")]),
            prim(PrimitiveName::String),
        ),
        // X | never (where X also reduces to never) → never.
        (
            union(vec![prim(PrimitiveName::Never), prim(PrimitiveName::Never)]),
            prim(PrimitiveName::Never),
        ),
        // X | never → X.
        (
            union(vec![
                prim(PrimitiveName::String),
                prim(PrimitiveName::Never),
            ]),
            prim(PrimitiveName::String),
        ),
        // X & unknown → X.
        (
            intersection(vec![
                prim(PrimitiveName::String),
                prim(PrimitiveName::Unknown),
            ]),
            prim(PrimitiveName::String),
        ),
        // ("a" | string) → string.
        (
            union(vec![slit("a"), prim(PrimitiveName::String)]),
            prim(PrimitiveName::String),
        ),
        // nested-union flatten + dedup + sort vs the flat canonical form.
        (
            union(vec![
                union(vec![
                    prim(PrimitiveName::Number),
                    prim(PrimitiveName::String),
                ]),
                prim(PrimitiveName::String),
            ]),
            union(vec![
                prim(PrimitiveName::String),
                prim(PrimitiveName::Number),
            ]),
        ),
        // X & never → never (absorbing in an intersection).
        (
            intersection(vec![
                prim(PrimitiveName::String),
                prim(PrimitiveName::Never),
            ]),
            prim(PrimitiveName::Never),
        ),
        // X | unknown → unknown (absorbing in a union).
        (
            union(vec![
                prim(PrimitiveName::String),
                prim(PrimitiveName::Unknown),
            ]),
            prim(PrimitiveName::Unknown),
        ),
    ];

    for (a, b) in &equal_pairs {
        assert_eq!(
            norm(a),
            norm(b),
            "equal-but-differently-spelled inputs must normalize byte-equal:\n  a = {a:?}\n  b = {b:?}"
        );
    }

    // Confluence must NOT over-collapse: genuinely-unequal inputs MUST diverge.
    let unequal_pairs: Vec<(TypeExpr, TypeExpr)> = vec![
        (
            union(vec![blit(true), blit(false), prim(PrimitiveName::Number)]),
            union(vec![
                prim(PrimitiveName::Boolean),
                prim(PrimitiveName::String),
            ]),
        ),
        // a literal-only union is a NARROWER set than the absorbing primitive.
        (
            union(vec![slit("a"), slit("b")]),
            prim(PrimitiveName::String),
        ),
        (prim(PrimitiveName::String), prim(PrimitiveName::Number)),
    ];
    for (a, b) in &unequal_pairs {
        assert_ne!(
            norm(a),
            norm(b),
            "unequal inputs must still diverge (confluence must not over-collapse)"
        );
    }
}

// =========================================================================
// oracle_normalization_discriminates
// =========================================================================

#[test]
fn oracle_normalization_discriminates() {
    let base = obj(vec![
        prop("id", prim(PrimitiveName::Number), false, false),
        prop("label", prim(PrimitiveName::String), false, false),
    ]);

    // wrong member type.
    let wrong_type = obj(vec![
        prop("id", prim(PrimitiveName::String), false, false),
        prop("label", prim(PrimitiveName::String), false, false),
    ]);
    assert_ne!(
        norm(&base),
        norm(&wrong_type),
        "wrong member type must diverge"
    );

    // wrong optionality.
    let wrong_opt = obj(vec![
        prop("id", prim(PrimitiveName::Number), true, false),
        prop("label", prim(PrimitiveName::String), false, false),
    ]);
    assert_ne!(
        norm(&base),
        norm(&wrong_opt),
        "wrong optionality must diverge"
    );

    // missing member.
    let missing = obj(vec![prop("id", prim(PrimitiveName::Number), false, false)]);
    assert_ne!(norm(&base), norm(&missing), "missing member must diverge");

    // overload ORDER is semantic — must NOT be sorted away (the two orders
    // diverge, proving order is preserved).
    let sig = |ret: PrimitiveName| {
        ObjectMember::Method(MethodSignature::synthetic_public(
            "f".to_string(),
            FunctionExpr::synthetic(vec![], Some(Arc::new(prim(ret))), vec![]),
            false,
        ))
    };
    let order_a = obj(vec![sig(PrimitiveName::Number), sig(PrimitiveName::String)]);
    let order_b = obj(vec![sig(PrimitiveName::String), sig(PrimitiveName::Number)]);
    assert_ne!(
        norm(&order_a),
        norm(&order_b),
        "overload order is semantic and must be preserved (not sorted)"
    );
}

// =========================================================================
// oracle_normalizer_terminates_on_cyclic_input  (reduction step 0)
// =========================================================================

#[test]
fn oracle_normalizer_terminates_on_cyclic_input() {
    // `type L = { next: L }` — Verter carries the back-edge as a RecursiveRef.
    let recursive = obj(vec![prop(
        "next",
        TypeExpr::RecursiveRef {
            name: Arc::from("L"),
            type_arguments: Arc::from(Vec::new()),
            conditional_context: Arc::from(Vec::new()),
        },
        false,
        false,
    )]);
    // Bounded: returns Ok with the RecursiveRef preserved as an opaque leaf.
    let normalized = normalize(&recursive, M).expect("cyclic input must normalize in bounded time");
    let TypeExpr::Object(o) = &normalized else {
        panic!("expected object");
    };
    let ObjectMember::Property(p) = &o.properties[0] else {
        panic!("expected property");
    };
    assert!(
        matches!(p.ty, TypeExpr::RecursiveRef { .. }),
        "RecursiveRef must stay an opaque leaf, never followed/expanded"
    );
    // Idempotent over the cyclic form too.
    assert_eq!(norm(&recursive), norm(&normalized));
}

// =========================================================================
// oracle_literal_spelling_canonicalized  (step 5)
// =========================================================================

#[test]
fn oracle_literal_spelling_canonicalized() {
    // Equal numeric literals (any construction path) normalize byte-equal; a
    // literal-only union is order-insensitive.
    let a = union(vec![nlit(1.0), nlit(2.0)]);
    let b = union(vec![nlit(2.0), nlit(1.0)]);
    assert_eq!(
        norm(&a),
        norm(&b),
        "same literal-only set, permuted, must be byte-equal"
    );

    // VALUE is preserved — 1.5 vs 1 still diverge.
    assert_ne!(
        norm(&nlit(1.5)),
        norm(&nlit(1.0)),
        "distinct literal VALUES must diverge"
    );

    // bigint spelling canonicalization (suffix `n`, leading zeros) — same value.
    let big_a = TypeExpr::Literal(LiteralValue::BigInt("007n".to_string()));
    let big_b = TypeExpr::Literal(LiteralValue::BigInt("7".to_string()));
    assert_eq!(
        norm(&big_a),
        norm(&big_b),
        "bigint spelling must canonicalize to one value"
    );
    // distinct bigint VALUES diverge.
    let big_c = TypeExpr::Literal(LiteralValue::BigInt("8".to_string()));
    assert_ne!(
        norm(&big_b),
        norm(&big_c),
        "distinct bigint values must diverge"
    );
}

// =========================================================================
// oracle_literal_subsumption_discriminates  (step 5)
// =========================================================================

#[test]
fn oracle_literal_subsumption_discriminates() {
    // "a" | string → string  AND  "b" | string → string  ⟹ compare EQUAL.
    let a = union(vec![slit("a"), prim(PrimitiveName::String)]);
    let b = union(vec![slit("b"), prim(PrimitiveName::String)]);
    assert_eq!(norm(&a), norm(&b), "both collapse to `string`");

    // "a" | string (→ string) vs "a" | "b" (literal-only union, NOT collapsed)
    // MUST diverge — the rule does not over-collapse a narrower set.
    let literal_only = union(vec![slit("a"), slit("b")]);
    assert_ne!(
        norm(&a),
        norm(&literal_only),
        "string must not equal the literal-only set"
    );

    // "a" | number (no co-present `string`) must NOT collapse the "a" arm.
    let no_base = union(vec![slit("a"), prim(PrimitiveName::Number)]);
    assert_ne!(
        norm(&no_base),
        norm(&prim(PrimitiveName::Number)),
        "subsumption fires only on the co-present base type"
    );
}

// =========================================================================
// oracle_normalization_canonicalizes_cosmetic_names  (step 6)
// =========================================================================

#[test]
fn oracle_normalization_canonicalizes_cosmetic_names() {
    // (a) index-signature parameter name is cosmetic: { [key: string]: T } and
    //     { [x: string]: T } are the SAME type.
    let with_key = obj(vec![idx("key", prim(PrimitiveName::Number))]);
    let with_x = obj(vec![idx("x", prim(PrimitiveName::Number))]);
    assert_eq!(
        norm(&with_key),
        norm(&with_x),
        "index-sig param name is cosmetic"
    );

    // (a) but the VALUE type is not cosmetic — a different value diverges.
    let with_str = obj(vec![idx("x", prim(PrimitiveName::String))]);
    assert_ne!(
        norm(&with_key),
        norm(&with_str),
        "index-sig value type is not cosmetic"
    );

    // (b) a generic type-parameter name is cosmetic AT THE BINDER + USE SITE: a
    //     fn<T>(p: T) and fn<U>(p: U) are the same type (both → T0 + Ref T0).
    let fn_t = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("p".to_string()),
            type_ref("T"),
            false,
            false,
        )],
        None,
        vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
    )));
    let fn_u = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("q".to_string()),
            type_ref("U"),
            false,
            false,
        )],
        None,
        vec![TypeParam {
            name: "U".to_string(),
            constraint: None,
            default: None,
        }],
    )));
    assert_eq!(
        norm(&fn_t),
        norm(&fn_u),
        "generic type-param name + use site is cosmetic"
    );

    // (b) a FREE Ref (no enclosing binder) is NOT a cosmetic binder name — it is
    //     a real alias reference and must be left UNCHANGED (so two distinct
    //     free aliases diverge).
    assert_ne!(
        norm(&type_ref("T")),
        norm(&type_ref("U")),
        "a free alias Ref binds to no in-scope binder and must not be renamed"
    );

    // (d) a TemplateLiteral cosmetic axis is default-rejected in the initial scope.
    let template = TypeExpr::TemplateLiteral {
        quasis: vec!["a".to_string(), "b".to_string()],
        expressions: Arc::from(vec![prim(PrimitiveName::String)]),
    };
    assert_eq!(
        normalized_canonical_json(&template, M),
        Err(NormalizeReject::TemplateLiteralCosmetic),
        "TemplateLiteral cosmetic axis is default-rejected until spiked"
    );
}

// =========================================================================
// binder_order_is_cross_side_stable  (step 6)
// =========================================================================

#[test]
fn binder_order_is_cross_side_stable() {
    // The positional T0,T1,… rename is confluent ONLY when both sides present
    // the SAME ordered binder list. This guard proves the rename binds
    // POSITIONALLY (so order matters): identical ordered binders → byte-equal;
    // a DIFFERENT binder order → a DIFFERENT normal form (which is exactly why
    // the admission gate must require cross-side binder-order stability — a
    // best-effort positional rename over divergent orders would manufacture
    // false parity).
    let make = |first: &str, second: &str, p0: &str, p1: &str| {
        TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![
                FunctionParam::synthetic(Some("a".to_string()), type_ref(p0), false, false),
                FunctionParam::synthetic(Some("b".to_string()), type_ref(p1), false, false),
            ],
            None,
            vec![
                TypeParam {
                    name: first.to_string(),
                    constraint: None,
                    default: None,
                },
                TypeParam {
                    name: second.to_string(),
                    constraint: None,
                    default: None,
                },
            ],
        )))
    };

    // Same ordered binder list, different source names, same use positions →
    // byte-equal (cosmetic rename is confluent).
    let stable_a = make("T", "U", "T", "U");
    let stable_b = make("X", "Y", "X", "Y");
    assert_eq!(
        norm(&stable_a),
        norm(&stable_b),
        "identical ordered binders rename byte-equal"
    );

    // A reordered binder list (p0 now uses the SECOND binder) is a DIFFERENT
    // type and MUST diverge — the rename does not collapse the difference.
    let reordered = make("T", "U", "U", "T");
    assert_ne!(
        norm(&stable_a),
        norm(&reordered),
        "a different binder order normalizes differently (positional rename does not mask it)"
    );
}

// =========================================================================
// Reject the genuinely-unrepresentable nodes (step 7).
// =========================================================================

#[test]
fn unknown_and_synthetic_nodes_reject() {
    let unknown = TypeExpr::Unknown {
        raw: "garbage".to_string(),
    };
    assert_eq!(
        normalized_canonical_json(&unknown, M),
        Err(NormalizeReject::UnknownNode),
        "Unknown is a comparison failure, not normalized away"
    );
}
