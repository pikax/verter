//! @ai-generated - contextual-typing contracts.
//!
//! Each test pins ONE TS7 emission for a contextual-typing scenario —
//! callback-parameter inference from a contextual signature, object-literal
//! contextual flow, return-type contextual flow, `as` casts (which erase
//! context), `as const` (which adds `readonly`), `satisfies` (which widens
//! to the satisfies target), array-literal contextual typing as tuple,
//! discriminated-union contextual narrowing, JSX-like attribute typing,
//! and contextual typing via type parameter constraints.
//!
//! Contextual typing is the TS7 mechanism for flowing the *expected* type
//! INTO an expression site rather than inferring it bottom-up — it is the
//! load-bearing inference primitive for Vue's `defineProps`, `defineEmits`,
//! JSX prop typing, and callback parameter inference.
//!
//! All emissions verified against tsgo 7.0.0-dev.20260523.1 via IsExactly
//! probes BEFORE encoding the Rust assertions.
//!
//! Each scenario is one `*Result = ReturnType<typeof ctXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.
//!
//! Documented TS7 surprises:
//!   * Ct09 — `const x: U = literal` is NARROWED to the assigned arm in
//!     the return position (NOT the declared union).
//!   * Ct10 — `[string, number]` flows onto the array literal producing a
//!     tuple emission, NOT a widened `(string|number)[]`.
//!   * Ct11 — `as const` adds a `readonly` modifier to every property.
//!   * Ct13 — `as { a: 1 }` on `{ a: 1, b: 2 }` narrows the shape, dropping
//!     excess properties.
//!   * Ct14 — `satisfies T` evaluates to T (the wider satisfies target),
//!     NOT the narrow literal source. The `const` keyword does NOT preserve
//!     literal types in this case.
//!
//! All scenarios are currently `#[ignore]` because typeinfo does not yet
//! propagate contextual-typing inference through `ReturnType<typeof fn>`.
//! Each test is the future contract for that specific emission.

use super::support::*;

const CONTEXTUAL_TYPING: &str = include_str!("fixtures/contextual_typing.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/contextual_typing.ts", CONTEXTUAL_TYPING);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/contextual_typing.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) Callback parameter from contextual signature ----------------
// TS7: `Array<number>.map<U>(cb: (x: number) => U)` flows `number` into
// the callback parameter `x`. `x.toFixed(2)` returns `string`. The map
// call returns `string[]`.
#[test]
#[ignore = "typeinfo currently does not propagate contextual-typing inference (number flowed into callback parameter via Array<number>.map signature) through `ReturnType<typeof fn>` to `string[]`; keep as the future Ct01 callback-parameter-from-contextual-signature contract"]
fn contextual_typing_ct01_callback_parameter_from_contextual_signature() {
    let expr = resolve_alias("Ct01Result");
    assert_array_of_primitive(&expr, PrimitiveName::String);
}

// ----- 2) Same callback, named to characterize the return type ---------
// TS7: identical structure to Ct01; pinned as the read-the-return-type
// contract. The function's return type is `string[]`.
#[test]
#[ignore = "typeinfo currently does not propagate contextual-typing inference through `ReturnType<typeof fn>` to the named function's `string[]` return type; keep as the future Ct02 read-callback-return-type contract"]
fn contextual_typing_ct02_callback_return_type_published() {
    let expr = resolve_alias("Ct02Result");
    assert_array_of_primitive(&expr, PrimitiveName::String);
}

// ----- 3) Object literal assignment from typed target ------------------
// TS7: `const o: { a: 1; b: 2 } = { a: 1, b: 2 }` preserves the literal
// types. `typeof o` is `{ a: 1; b: 2 }`.
#[test]
#[ignore = "typeinfo currently does not propagate contextual typing from a declared object-literal target type through `ReturnType<typeof fn>` to preserve literal property types; keep as the future Ct03 object-literal-assignment-from-typed-target contract"]
fn contextual_typing_ct03_object_literal_assignment_from_typed_target() {
    let expr = resolve_alias("Ct03Result");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert_number_literal(&props["a"].ty, 1.0);
    assert_number_literal(&props["b"].ty, 2.0);
}

// ----- 4) Object literal in function call ------------------------------
// TS7: `ct04(o: { tag: "x" })` contextually types the argument literal
// as `{ tag: "x" }`. Returning `o` publishes that contextually-typed
// shape — emission is `{ tag: "x" }`.
#[test]
#[ignore = "typeinfo currently does not propagate contextual typing from a function parameter type onto an object-literal argument through `ReturnType<typeof fn>`; keep as the future Ct04 object-literal-in-function-call contract"]
fn contextual_typing_ct04_object_literal_in_function_call() {
    let expr = resolve_alias("Ct04Result");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["tag"]);
    assert_string_literal(&props["tag"].ty, "x");
}

// ----- 5) Return-type contextual flow ----------------------------------
// TS7: `const fn05: () => 42 = () => 42` — the declared return type `42`
// is contextually applied to the arrow body. Calling `fn05()` returns `42`,
// NOT `number`.
#[test]
fn contextual_typing_ct05_return_type_contextual_flow() {
    let expr = resolve_alias("Ct05Result");
    assert_number_literal(&expr, 42.0);
}

// ----- 6) Parenthesized expression preserves context -------------------
// TS7: wrapping the arrow in parens does NOT erase contextual typing.
// `const fn06: () => 42 = (() => 42)` still returns `42`.
#[test]
fn contextual_typing_ct06_parenthesized_expression_preserves_context() {
    let expr = resolve_alias("Ct06Result");
    assert_number_literal(&expr, 42.0);
}

// ----- 7) `as` cast erases context -------------------------------------
// TS7: `const fn07: () => number = () => 42 as number` — the `as` cast
// widens the body to `number`. Calling `fn07()` returns `number`, NOT
// the literal `42`.
#[test]
#[ignore = "typeinfo currently does not propagate `as`-cast widening on a contextually-typed body through `ReturnType<typeof fn>` to the widened `number`; keep as the future Ct07 as-cast-erases-context contract"]
fn contextual_typing_ct07_as_cast_erases_context() {
    let expr = resolve_alias("Ct07Result");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 8) JSX-like attribute contextual typing -------------------------
// TS7: `take(p: Props)` flows the `Props = { count: 1 }` shape onto the
// argument literal. `take({ count: 1 })` returns `p.count` (the literal
// `1`). Emission is `1`.
#[test]
#[ignore = "typeinfo currently does not propagate contextual typing from a function parameter type onto an object-literal argument, then through property access and `ReturnType<typeof fn>` to the literal `1`; keep as the future Ct08 jsx-like-attribute-contextual-typing contract"]
fn contextual_typing_ct08_jsx_like_attribute_contextual_typing() {
    let expr = resolve_alias("Ct08Result");
    assert_number_literal(&expr, 1.0);
}

// ----- 9) Discriminated union contextual (with narrowing) --------------
// TS7 quirk: `const x: U = { kind: "a", a: 1 }` is NARROWED in the return
// position to the arm of the assigned literal — emission is the narrow
// arm `{ kind: "a"; a: 1 }`, NOT the declared union.
#[test]
#[ignore = "typeinfo currently does not propagate contextual typing from a discriminated-union declared type combined with TS7 same-block narrowing in the return position through `ReturnType<typeof fn>` to the narrow arm; keep as the future Ct09 discriminated-union-contextual-narrowing contract"]
fn contextual_typing_ct09_discriminated_union_contextual_narrowing() {
    let expr = resolve_alias("Ct09Result");
    // TS7 narrows to the assigned arm: { kind: "a"; a: 1 }.
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "kind"]);
    assert_number_literal(&props["a"].ty, 1.0);
    assert_string_literal(&props["kind"].ty, "a");
}

// ----- 10) Array literal contextually typed as tuple -------------------
// TS7 quirk: `const t: [string, number] = ["a", 1]` — the array literal
// is contextually typed as a tuple. Emission is `[string, number]`,
// NOT `(string | number)[]`.
#[test]
#[ignore = "typeinfo currently does not propagate tuple-type contextual typing on an array literal initializer through `ReturnType<typeof fn>` to the tuple shape `[string, number]`; keep as the future Ct10 array-literal-contextually-typed-as-tuple contract"]
fn contextual_typing_ct10_array_literal_contextually_typed_as_tuple() {
    let expr = resolve_alias("Ct10Result");
    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple type, got {expr:?}");
    };
    assert_eq!(elements.len(), 2);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
}

// ----- 11) `as const` overrides contextual widening --------------------
// TS7 quirk: `{ a: 1 } as const` produces `{ readonly a: 1 }` — every
// property is marked readonly. The literal type `1` is preserved.
#[test]
#[ignore = "typeinfo currently does not propagate `as const` on an object literal through `ReturnType<typeof fn>` to the `{ readonly a: 1 }` emission including the readonly modifier; keep as the future Ct11 as-const-readonly-modifier contract"]
fn contextual_typing_ct11_as_const_readonly_modifier() {
    let expr = resolve_alias("Ct11Result");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a"]);
    assert_number_literal(&props["a"].ty, 1.0);
    // `as const` MUST mark `a` as readonly. This is the load-bearing
    // emission detail this scenario pins.
    assert!(
        props["a"].readonly,
        "`as const` must mark every property readonly; got {:?}",
        &props["a"]
    );
}

// ----- 12) Function expression argument type from contextual signature -
// TS7: `E<number>` flows onto `e`, which contextually types the callback
// `f`'s parameter `x` as `number`. `e(x => x.toFixed(2))` returns `string`.
#[test]
#[ignore = "typeinfo currently does not propagate generic-instantiated contextual typing through a nested callback parameter and `ReturnType<typeof fn>` to `string`; keep as the future Ct12 function-expression-argument-from-contextual-signature contract"]
fn contextual_typing_ct12_function_expression_argument_from_contextual_signature() {
    let expr = resolve_alias("Ct12Result");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 13) Object literal `as` cast narrows shape ----------------------
// TS7 quirk: `{ a: 1, b: 2 } as { a: 1 }` narrows to the cast target,
// dropping the excess `b` property. Emission is `{ a: 1 }`.
#[test]
#[ignore = "typeinfo currently does not propagate `as` cast on an object literal to narrow the shape (drop excess properties) through `ReturnType<typeof fn>` to `{ a: 1 }`; keep as the future Ct13 object-literal-as-cast-narrows-shape contract"]
fn contextual_typing_ct13_object_literal_as_cast_narrows_shape() {
    let expr = resolve_alias("Ct13Result");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a"]);
    assert_number_literal(&props["a"].ty, 1.0);
}

// ----- 14) `satisfies` operator widens to satisfies target -------------
// TS7 quirk: `satisfies T` evaluates to T (the wider satisfies target),
// NOT the narrow literal source. Emission is `{ a: number; b: string }`.
// The `const` keyword does NOT preserve literal types here — this is
// the key surprise satisfies pins.
#[test]
#[ignore = "typeinfo currently does not propagate `satisfies T` on an object literal to the wider T target shape through `ReturnType<typeof fn>` to `{ a: number; b: string }`; keep as the future Ct14 satisfies-widens-to-target contract"]
fn contextual_typing_ct14_satisfies_widens_to_target() {
    let expr = resolve_alias("Ct14Result");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    // `satisfies` evaluates to the target type: { a: number; b: string }.
    assert_primitive(&props["a"].ty, PrimitiveName::Number);
    assert_primitive(&props["b"].ty, PrimitiveName::String);
}

// ----- 15) Contextual type via type parameter constraint --------------
// TS7: `call15<T>(f: (x: T) => T, x: T): T` — TS infers T from the
// second argument (1 -> number), then contextually types the callback
// parameter x as number. x + 1 returns number. The outer call returns
// T = number.
#[test]
#[ignore = "typeinfo currently does not propagate generic-inference-driven contextual typing (T inferred from a value argument, then flowed into a callback parameter) through `ReturnType<typeof fn>` to `number`; keep as the future Ct15 contextual-type-via-type-parameter-constraint contract"]
fn contextual_typing_ct15_contextual_type_via_type_parameter_constraint() {
    let expr = resolve_alias("Ct15Result");
    assert_primitive(&expr, PrimitiveName::Number);
}
