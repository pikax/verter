//! @ai-generated - `"prop" in x` narrowing contracts.
//!
//! Each test pins ONE TS7 emission for `"prop" in x` narrowing — TS's
//! structural-discriminator narrowing primitive. Distinct from
//! `narrow_discriminated_union.rs` (which uses VALUE-discriminated unions
//! with a `kind` field): `in` narrows by PROPERTY PRESENCE. Scenarios cover
//! binary unions, shared-property unions, else-branch reads, intersections,
//! optional properties, `unknown` + `object`, compound `&&`, negation,
//! three-arm unions, generic-constrained operands, reassignment re-narrowing,
//! class vs object literal, template-literal-typed keys (which DO NOT
//! narrow), and `Symbol.iterator`.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof ioXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const NARROW_IN_OPERATOR: &str = include_str!("fixtures/narrow_in_operator.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/narrow_in_operator.ts", NARROW_IN_OPERATOR);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_in_operator.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) "a" in x on {a:string} | {b:number} --------------------------
// TS7: if-branch sees {a:string} arm, else sees {b:number} arm. Joined:
// string | number.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing on a binary structural union through `ReturnType<typeof fn>`; keep as the future Io01 in-operator binary-union contract"]
fn narrow_in_operator_io01_binary_union() {
    let expr = resolve_alias("Io01Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 2) "a" in x on {a:string} | {a:number;b:1} ----------------------
// TS7 quirk: both arms have `a`, so the guard does NOT discriminate.
// Branch sees full union; x.a is `string | number`. Else is never (absorbed).
// Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate the TS7 non-discriminating `\"a\" in x` emission when both union arms carry the probed key through `ReturnType<typeof fn>`; keep as the future Io02 in-operator shared-key contract"]
fn narrow_in_operator_io02_shared_key() {
    let expr = resolve_alias("Io02Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 3) "a" in x else-branch — same shape as #1 but read else. --------
// !("a" in x) selects {b:number}; positive branch selects {a:string}.
// Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate the else-branch of `\"a\" in x` narrowing through `ReturnType<typeof fn>`; keep as the future Io03 in-operator else-branch contract"]
fn narrow_in_operator_io03_else_branch() {
    let expr = resolve_alias("Io03Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 4) "a" in x on intersection — `a` always present. ---------------
// Branch is full intersection, else is never (absorbed). Joined:
// {a:string} & {b:number}.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing on an intersection (a is always present, branch is full intersection) through `ReturnType<typeof fn>`; keep as the future Io04 in-operator intersection contract"]
fn narrow_in_operator_io04_intersection() {
    let expr = resolve_alias("Io04Result");
    // Joined: an intersection with `a` and `b`.
    let props = object_props(&expr);
    let names = prop_names(&props);
    assert!(
        names.contains(&"a") && names.contains(&"b"),
        "expected intersection with a + b props, got {expr:?}"
    );
}

// ----- 5) Optional property `a?: string` -------------------------------
// TS7 quirk: `"a" in x` does NOT narrow `a` to non-undefined for an
// optional declared property. Branch returns `x.a` which is
// `string | undefined`. Else is never (absorbed).
// Joined: string | undefined.
#[test]
#[ignore = "typeinfo currently does not propagate the TS7 optional-property emission for `\"a\" in x` (branch keeps `string | undefined`) through `ReturnType<typeof fn>`; keep as the future Io05 in-operator optional-property contract"]
fn narrow_in_operator_io05_optional_property() {
    let expr = resolve_alias("Io05Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Undefined);
}

// ----- 6) "a" in x on unknown — requires typeof+null widening ----------
// TS7 quirk: `in` on a raw `unknown` is a type error. After widening via
// `typeof x === "object" && x !== null`, `"a" in x` narrows the branch to
// `object & Record<"a", unknown>`. Else returns null.
// Joined: (object & Record<"a", unknown>) | null.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing on `unknown` (with typeof+null widening) to `object & Record<\"a\", unknown>` through `ReturnType<typeof fn>`; keep as the future Io06 in-operator on-unknown contract"]
fn narrow_in_operator_io06_on_unknown() {
    let expr = resolve_alias("Io06Result");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_intersection = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Intersection(_)));
    assert!(
        has_intersection,
        "joined return must contain an intersection arm for `object & Record<\"a\", unknown>`; got {expr:?}"
    );
}

// ----- 7) "a" in x on object — narrows to object & Record<"a", unknown>.
// Else returns null. Joined: (object & Record<"a", unknown>) | null.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing on `object` to `object & Record<\"a\", unknown>` through `ReturnType<typeof fn>`; keep as the future Io07 in-operator on-object contract"]
fn narrow_in_operator_io07_on_object() {
    let expr = resolve_alias("Io07Result");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_intersection = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Intersection(_)));
    assert!(
        has_intersection,
        "joined return must contain an intersection arm for `object & Record<\"a\", unknown>`; got {expr:?}"
    );
}

// ----- 8) compound "a" in x && "b" in x --------------------------------
// Branch narrows to the arm with both keys ({a:1; b:2}). Else returns null.
// Joined: {a:1; b:2} | null.
#[test]
#[ignore = "typeinfo currently does not propagate compound `\"a\" in x && \"b\" in x` narrowing through `ReturnType<typeof fn>`; keep as the future Io08 in-operator compound-conjunction contract"]
fn narrow_in_operator_io08_compound_conjunction() {
    let expr = resolve_alias("Io08Result");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    assert_union_has_object_arm(&expr, &["a", "b"]);
}

// ----- 9) !("a" in x) negated ------------------------------------------
// Same emission as #1 with branch/else swapped. Joined: string | number.
#[test]
#[ignore = "typeinfo currently does not propagate negated `!(\"a\" in x)` narrowing through `ReturnType<typeof fn>`; keep as the future Io09 in-operator negated contract"]
fn narrow_in_operator_io09_negated() {
    let expr = resolve_alias("Io09Result");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 10) Three-arm union: {a:1}|{a:2;b:1}|{c:1} ----------------------
// Branch = {a:1} | {a:2;b:1}; x.a is `1 | 2`. Else = {c:1}; x.c is `1`.
// Joined: 1 | 2.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing on a three-arm structural union through `ReturnType<typeof fn>`; keep as the future Io10 in-operator three-arm contract"]
fn narrow_in_operator_io10_three_arm_union() {
    let expr = resolve_alias("Io10Result");
    assert_number_literal_union(&expr, &[1.0, 2.0]);
}

// ----- 11) Generic constrained to Record<string, unknown> --------------
// At the ReturnType call site, T resolves to its constraint:
// Record<string, unknown>. Branch returns x; else throws (never, absorbed).
// Joined: Record<string, unknown> — projects as an Object with a string
// index signature mapping to `unknown`.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing on a constrained generic (T extends Record<string, unknown>) through `ReturnType<typeof fn>`; keep as the future Io11 in-operator generic-constrained contract"]
fn narrow_in_operator_io11_generic_constrained() {
    let expr = resolve_alias("Io11Result");
    let sigs = object_index_signatures(&expr);
    assert!(
        sigs.iter().any(|sig| matches!(
            (&sig.key_type, &sig.value_type),
            (
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Unknown)
            )
        )),
        "expected string -> unknown index signature, got {sigs:?} from {expr:?}"
    );
}

// ----- 12) Reassignment in branch --------------------------------------
// Inside the if-branch, `x = { b: 2 }` re-narrows x to {b:2}. Post-assignment
// read of x.b is `2`. Else also reads x.b from original {b:2} arm — also `2`.
// Joined: 2.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` re-narrowing after reassignment in the branch through `ReturnType<typeof fn>`; keep as the future Io12 in-operator reassignment-renarrowing contract"]
fn narrow_in_operator_io12_reassignment_renarrowing() {
    let expr = resolve_alias("Io12Result");
    assert_eq!(
        expr,
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(2.0))
    );
}

// ----- 13) Class instance vs object literal ----------------------------
// Branch narrows to Io13C (the class has field `a`). Else returns x — the
// branch and else returns are joined as Io13C | {b:2}.
#[test]
#[ignore = "typeinfo currently does not propagate `\"a\" in x` narrowing across class-instance vs object-literal union through `ReturnType<typeof fn>`; keep as the future Io13 in-operator class-vs-object contract"]
fn narrow_in_operator_io13_class_vs_object() {
    let expr = resolve_alias("Io13Result");
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_class = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "Io13C"));
    assert!(
        has_class,
        "joined return must contain Io13C arm; got {expr:?}"
    );
}

// ----- 14) Template-literal-typed key — TS7 does NOT narrow ------------
// `key: \`prefixed_${string}\`` is a template-literal type with a generic
// placeholder. `key in x` does NOT narrow. Both branches return the same
// union. Joined: {prefixed_a:1} | {other:2}.
#[test]
#[ignore = "typeinfo currently does not characterize the TS7 NO-OP emission for a template-literal-typed key in `key in x` through `ReturnType<typeof fn>`; keep as the future Io14 in-operator template-literal-key contract"]
fn narrow_in_operator_io14_template_literal_key() {
    let expr = resolve_alias("Io14Result");
    // Joined: union of two object literals — one carrying `prefixed_a`,
    // the other carrying `other`. The two-arm union must be preserved
    // because TS7 did not narrow.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_prefixed = arms.iter().any(|arm| match arm {
        TypeExpr::Object(_) => prop_names(&object_props(arm)) == ["prefixed_a"],
        _ => false,
    });
    let has_other = arms.iter().any(|arm| match arm {
        TypeExpr::Object(_) => prop_names(&object_props(arm)) == ["other"],
        _ => false,
    });
    assert!(
        has_prefixed && has_other,
        "joined return must contain both {{prefixed_a:1}} and {{other:2}} arms (no narrowing); got {expr:?}"
    );
}

// ----- 15) Symbol.iterator in x ---------------------------------------
// Branch narrows to the iterable arm; else returns null.
// Joined: { [Symbol.iterator](): Iterator<number> } | null.
#[test]
#[ignore = "typeinfo currently does not propagate `Symbol.iterator in x` narrowing through `ReturnType<typeof fn>`; keep as the future Io15 in-operator symbol-key contract"]
fn narrow_in_operator_io15_symbol_key() {
    let expr = resolve_alias("Io15Result");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_iterable = arms.iter().any(|arm| matches!(arm, TypeExpr::Object(_)));
    assert!(
        has_iterable,
        "joined return must contain an object arm with a Symbol.iterator method; got {expr:?}"
    );
}
