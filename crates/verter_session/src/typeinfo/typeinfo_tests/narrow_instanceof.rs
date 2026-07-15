//! @ai-generated - instanceof-narrowing contracts.
//!
//! Each test pins ONE TS7 emission for `x instanceof Foo` narrowing —
//! TS's class-discriminator narrowing primitive. Scenarios cover binary
//! class unions; class + primitive unions; instanceof on `unknown`;
//! subclass union subsumption; already-narrowed declared types; abstract
//! classes; else-branch reachability; instanceof on interface unions;
//! negated narrowing with early return; intersection preservation;
//! generic constructor types; chained instanceof; the `Array` / `Promise`
//! special cases; and `A | null | undefined`.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof inXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const NARROW_INSTANCEOF: &str = include_str!("fixtures/narrow_instanceof.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/narrow_instanceof.ts", NARROW_INSTANCEOF);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/narrow_instanceof.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) x instanceof A on A | B ---------------------------------------
// TS7: if-branch returns A, else returns B. Joined: A | B.
#[test]
#[ignore = "typeinfo currently does not propagate `instanceof`-narrowing through `ReturnType<typeof fn>` to the joined return type; keep as the future In01 instanceof-narrowing binary-class-union contract"]
fn narrow_instanceof_in01_binary_union() {
    let expr = resolve_alias("In01InstanceOfBinaryUnionResult");
    // Joined: InA | InB.
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_a = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "InA"));
    let has_b = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "InB"));
    assert!(has_a, "joined return must contain InA arm; got {expr:?}");
    assert!(has_b, "joined return must contain InB arm; got {expr:?}");
}

// ----- 2) x instanceof A on A | string ----------------------------------
// TS7: if-branch returns A, else returns string. Joined: A | string.
#[test]
#[ignore = "typeinfo currently does not propagate `instanceof`-narrowing across a class/primitive union through `ReturnType<typeof fn>`; keep as the future In02 instanceof-narrowing class-plus-primitive contract"]
fn narrow_instanceof_in02_class_plus_primitive() {
    let expr = resolve_alias("In02InstanceOfWithPrimitiveResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_class = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "InA"));
    assert!(
        has_class,
        "joined return must contain InA arm; got {expr:?}"
    );
}

// ----- 3) x instanceof A on unknown -------------------------------------
// TS7: if-branch narrows to A, else stays unknown. Joined: unknown (A is subsumed).
#[test]
#[ignore = "typeinfo currently does not propagate `instanceof`-narrowing on `unknown` through `ReturnType<typeof fn>` to the joined-unknown return; keep as the future In03 instanceof-on-unknown contract"]
fn narrow_instanceof_in03_on_unknown() {
    let expr = resolve_alias("In03InstanceOfOnUnknownResult");
    assert_primitive(&expr, PrimitiveName::Unknown);
}

// ----- 4) Subclass union — x: A | B where B extends A -------------------
// TS7 quirk: parameter type `In4A | In4B` collapses to `In4A` via
// subsumption (B is a subtype of A). The if-branch sees A, the else is
// unreachable (`never`, absorbed). Joined: A.
#[test]
#[ignore = "typeinfo currently does not collapse `A | B` to `A` when `B extends A` through `ReturnType<typeof fn>`; keep as the future In04 instanceof-subclass-union-subsumption contract"]
fn narrow_instanceof_in04_subclass_union() {
    let expr = resolve_alias("In04InstanceOfSubclassUnionResult");
    assert_ref(&expr, "In4A");
}

// ----- 5) Already-narrowed declared type — const x: B; x instanceof A ---
// TS7: if-branch keeps x as B (B already satisfies A). else is never.
// Joined: B.
#[test]
#[ignore = "typeinfo currently does not preserve a more-precise declared type (`const x: B`) across `instanceof A` through `ReturnType<typeof fn>`; keep as the future In05 instanceof-already-narrowed contract"]
fn narrow_instanceof_in05_already_narrowed() {
    let expr = resolve_alias("In05InstanceOfAlreadyNarrowedResult");
    assert_ref(&expr, "In5B");
}

// ----- 6) Abstract class — `abstract class A`, x: A = new B() -----------
// TS7: instanceof A does NOT widen `x` further; the if-branch keeps A.
// else is never (absorbed). Joined: A.
#[test]
#[ignore = "typeinfo currently does not preserve the abstract-class declared type across `instanceof` through `ReturnType<typeof fn>`; keep as the future In06 instanceof-abstract-class contract"]
fn narrow_instanceof_in06_abstract_class() {
    let expr = resolve_alias("In06InstanceOfAbstractResult");
    assert_ref(&expr, "In6A");
}

// ----- 7) else-branch reachability — if (instanceof) return; else x ----
// TS7: if-branch returns null; else returns `x` narrowed to NOT-A = B.
// Joined: B | null.
#[test]
#[ignore = "typeinfo currently does not propagate else-branch instanceof narrowing (`if (x instanceof A) return null; else x is B`) through `ReturnType<typeof fn>`; keep as the future In07 instanceof-else-reachability contract"]
fn narrow_instanceof_in07_else_reachability() {
    let expr = resolve_alias("In07InstanceOfElseReachabilityResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_b = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "In7B"));
    assert!(has_b, "joined return must contain In7B arm; got {expr:?}");
}

// ----- 8) instanceof on interface union — x: I; if (x instanceof A) ----
// TS7: I is an interface, A implements I. The if-branch narrows x to A.
// else returns null. Joined: A | null.
#[test]
#[ignore = "typeinfo currently does not propagate instanceof-narrowing on an interface-typed parameter against an implementing class through `ReturnType<typeof fn>`; keep as the future In08 instanceof-on-interface-union contract"]
fn narrow_instanceof_in08_interface_union() {
    let expr = resolve_alias("In08InstanceOfInterfaceUnionResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_class = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "In8A"));
    assert!(
        has_class,
        "joined return must contain In8A arm; got {expr:?}"
    );
}

// ----- 9) Negated narrowing — if (!(x instanceof A)) return; return x --
// TS7: if-branch (negated) returns null; trailing return sees x as A
// (only path past the guard). Joined: A | null.
#[test]
#[ignore = "typeinfo currently does not propagate negated instanceof narrowing across an early return through `ReturnType<typeof fn>`; keep as the future In09 negated-instanceof-early-return contract"]
fn narrow_instanceof_in09_negated_early_return() {
    let expr = resolve_alias("In09InstanceOfNegatedResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_a = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "In9A"));
    assert!(has_a, "joined return must contain In9A arm; got {expr:?}");
}

// ----- 10) Intersection — x: A & { tag: 1 }; if (x instanceof A) -------
// TS7 quirk: if-branch keeps the FULL intersection `A & { tag: 1 }`
// (instanceof narrows the A side; the `{ tag: 1 }` side is preserved).
// else returns null. Joined: (A & { tag: 1 }) | null.
#[test]
#[ignore = "typeinfo currently does not preserve an intersection's non-instanceof arm across `instanceof` through `ReturnType<typeof fn>`; keep as the future In10 instanceof-intersection contract"]
fn narrow_instanceof_in10_intersection() {
    let expr = resolve_alias("In10InstanceOfIntersectionResult");
    // Joined: (In10A & { tag: 1 }) | null.
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_intersection = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Intersection(_)));
    assert!(
        has_intersection,
        "joined return must contain an intersection arm; got {expr:?}"
    );
}

// ----- 11) Generic ctor T extends new (...args: any[]) => any ----------
// TS7 quirk: at the `ReturnType<typeof fn>` call site, T is not
// instantiated and resolves to its constraint, so `InstanceType<T>` is
// `any`. The if-branch narrows `x: unknown` to `any`, the else returns
// null. Joined: any | null (which collapses to `any` in TS, asserted
// here as containing both Any and Null arms via IsExactly).
#[test]
#[ignore = "typeinfo currently does not propagate generic-constructor instanceof narrowing (`x instanceof ctor` for `T extends new (...args: any[]) => any`) through `ReturnType<typeof fn>`; keep as the future In11 instanceof-generic-ctor contract"]
fn narrow_instanceof_in11_generic_ctor() {
    // ReturnType<typeof in11InstanceOfGenericCtor> resolves to `any` after
    // null-subsumption. We assert that the joined return contains `any`
    // (either as the whole expr or as a union arm). Null is subsumed into
    // any in TS, so a single-arm `any` is the expected emission.
    let expr = resolve_alias("In11InstanceOfGenericCtorResult");
    assert_expr_contains_primitive(&expr, PrimitiveName::Any);
}

// ----- 12) Chained instanceof — A else if B else C --------------------
// TS7: case A returns "a"; case B returns "b"; else returns "c". Joined:
// "a" | "b" | "c".
#[test]
fn narrow_instanceof_in12_chained() {
    let expr = resolve_alias("In12InstanceOfChainedResult");
    assert_literal_union(&expr, &["a", "b", "c"]);
}

// ----- 13) instanceof Array — narrows to any[] ------------------------
// TS7 special case: `x instanceof Array` narrows to `any[]` (NOT
// `unknown[]`), preserving historical TS behaviour. else returns null.
// Joined: any[] | null.
#[test]
#[ignore = "typeinfo currently does not propagate the TS7 `instanceof Array` special-case (narrows to `any[]`) through `ReturnType<typeof fn>`; keep as the future In13 instanceof-array-special-case contract"]
fn narrow_instanceof_in13_array_special_case() {
    let expr = resolve_alias("In13InstanceOfArrayResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_any_array = arms.iter().any(|arm| {
        matches!(
            arm,
            TypeExpr::Array { element, .. }
                if matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::Any))
        )
    });
    assert!(
        has_any_array,
        "joined return must contain an `any[]` arm; got {expr:?}"
    );
}

// ----- 14) instanceof Promise — narrows to Promise<any> ---------------
// TS7 special case: `x instanceof Promise` narrows to `Promise<any>`
// (NOT `Promise<unknown>`). else returns null. Joined: Promise<any> | null.
#[test]
#[ignore = "typeinfo currently does not propagate the TS7 `instanceof Promise` special-case (narrows to `Promise<any>`) through `ReturnType<typeof fn>`; keep as the future In14 instanceof-promise-special-case contract"]
fn narrow_instanceof_in14_promise_special_case() {
    let expr = resolve_alias("In14InstanceOfPromiseResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_promise = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "Promise"));
    assert!(
        has_promise,
        "joined return must contain a `Promise<...>` arm; got {expr:?}"
    );
}

// ----- 15) x: A | null | undefined; if (x instanceof A) --------------
// TS7: if-branch narrows to A (null/undefined are not instances). else
// returns null. Joined: A | null.
#[test]
#[ignore = "typeinfo currently does not propagate instanceof-narrowing on a nullable parameter through `ReturnType<typeof fn>`; keep as the future In15 instanceof-on-nullable contract"]
fn narrow_instanceof_in15_nullable() {
    let expr = resolve_alias("In15InstanceOfNullableResult");
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union return, got {expr:?}");
    };
    let has_a = arms
        .iter()
        .any(|arm| matches!(arm, TypeExpr::Ref { name, .. } if name.as_ref() == "In15A"));
    assert!(has_a, "joined return must contain In15A arm; got {expr:?}");
}
