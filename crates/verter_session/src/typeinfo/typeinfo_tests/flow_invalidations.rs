//! @ai-generated - Narrowing-invalidation contracts.
//!
//! Complement to `flow_return_catalog.rs`: each test pins a TS7 trap case
//! where narrowing is GAINED then LOST (or surprisingly PRESERVED), so the
//! resolver's CFG model must distinguish reassignment, opaque-call escape,
//! closure capture, destructured-discriminant correlation, finally-return
//! override, asserts-on-dotted-paths, and `never`-returning exhaustive
//! tails.
//!
//! Each scenario is one `*Result = ReturnType<typeof fnXX>` alias in the
//! fixture. The Rust test resolves that alias and asserts the TS7 emission.

use super::support::*;

const FLOW_INVALIDATIONS: &str = include_str!("fixtures/flow_invalidations.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/flow_invalidations.ts", FLOW_INVALIDATIONS);
}

fn resolve_alias(alias: &str) -> TypeExpr {
    let host = make_host_with_footprint();
    upsert(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/flow_invalidations.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    expr
}

// ----- 1) Reassignment invalidates the `string` narrowing ----------------
// TS7: after `x = 1` inside the `typeof x === "string"` branch the local
// flow narrows to `number` (literal `1` widened to its declared union arm).
// The else branch is `number`. Joined return: `number`.
#[test]
#[ignore = "typeinfo currently does not invalidate string-narrowing on a same-scope reassignment of the narrowed local; keep as the future Fi01 reassignment-invalidation contract"]
fn flow_invalidations_fi01_reassignment_invalidates_string_narrowing() {
    let expr = resolve_alias("Fi01ReassignInvalidatesResult");
    assert_primitive(&expr, PrimitiveName::Number);
}

// ----- 2) Narrowing PRESERVED across opaque call -------------------------
// TS7: an opaque (`unknown`-returning) function call does NOT invalidate
// the local narrowing. The if-branch returns the narrowed `string`; the
// else returns `number`. Joined: `string | number`.
#[test]
#[ignore = "typeinfo currently does not preserve the local string-narrowing across an opaque call before the return point; keep as the future Fi02 opaque-call preservation contract"]
fn flow_invalidations_fi02_narrowing_preserved_across_opaque_call() {
    let expr = resolve_alias("Fi02PreservedAcrossCallResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 3) Narrowing under closure capture --------------------------------
// TS7 mirrors LR10/CF11: the synchronous return point still sees the
// captured local as `string`. Closure capture + mutation in the callback
// body does NOT invalidate the narrowing at the immediate return. Else
// branch returns `number`. Joined: `string | number`.
//
// Verter currently fails the same way it fails LR10/CF11 — the entire
// narrowed-local CFG model needs the closure-capture invariant. This is
// a deliberate cross-check against LR10/CF11 through a slightly different
// fixture shape (parameter callback vs `let` initialiser).
#[test]
#[ignore = "typeinfo currently does not preserve the narrowed local return type after registering a capturing callback (mirrors LR10/CF11); keep as the future Fi03 closure-capture preservation contract"]
fn flow_invalidations_fi03_closure_capture_preserves_narrowing_at_return() {
    let expr = resolve_alias("Fi03CaptureInvalidatesResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 4) Destructured-discriminant PRESERVES correlation ----------------
// TS5+ propagates discriminated-union narrowing through a destructured
// local. `const { kind } = s; if (kind === "a") { /* s narrowed */ }`
// returns `string` for the if-branch and `number` for the else. Joined:
// `string | number`.
#[test]
#[ignore = "typeinfo currently does not propagate discriminated-union narrowing through `const { kind } = s` correlation (mirrors CN16); keep as the future Fi04 destructured-discriminant preservation contract"]
fn flow_invalidations_fi04_destructured_discriminant_preserves_correlation() {
    let expr = resolve_alias("Fi04DestructPreservesResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}

// ----- 5) Destructured-discriminant LOSES correlation after reassignment --
// `let { kind } = s; kind = "b"` breaks the destructured-discriminant
// link. At `kind === "a"` the source `s` is the unaltered `Fi04Shape`
// union (NOT the `kind: "a"` arm). The function returns `s` in both
// branches, so the joined return type is the full `Fi04Shape` union,
// proving narrowing was lost. The result is a 2-arm union of object arms.
#[ignore = "typeinfo currently resolves `ReturnType<typeof fi05DestructLoses>` to a 2-arm union of `Unknown { raw: \"semanticMiss\" }` arms rather than the structural `{ kind: \"a\"; a: string } | { kind: \"b\"; b: number }` shape: the typeof-of-function-with-destructured-binding ReturnType pipeline is not materialising the parameter's `Fi04Shape` union arms; keep as the future Fi05 destructured-reassignment ReturnType structural-arm contract"]
#[test]
fn flow_invalidations_fi05_destructured_discriminant_loses_on_reassignment() {
    let expr = resolve_alias("Fi05DestructLosesResult");
    // Joined: `{ kind: "a"; a: string } | { kind: "b"; b: number }`.
    assert_union_has_object_arm(&expr, &["a", "kind"]);
    assert_union_has_object_arm(&expr, &["b", "kind"]);
}

// ----- 6) `finally { return ... }` overrides try / catch returns --------
// TS7: when finally has a top-level return statement, every preceding try
// / catch return is unreachable for type-inference purposes. The function
// returns only `"from-finally"`.
#[test]
#[ignore = "typeinfo currently does not model finally-return as overriding preceding try/catch returns; keep as the future Fi06 finally-override contract"]
fn flow_invalidations_fi06_finally_return_overrides_try_catch_returns() {
    let expr = resolve_alias("Fi06FinallyOverridesResult");
    assert_string_literal(&expr, "from-finally");
}

// ----- 7) `finally { ... }` without a return preserves try/catch returns -
// TS7: a finally block that does NOT contain a top-level return does NOT
// override. The function's inferred return is `"from-try" | "from-catch"`.
#[test]
#[ignore = "typeinfo currently does not preserve try/catch returns when finally contains a non-return statement (mirrors CF06); keep as the future Fi07 finally-preserve contract"]
fn flow_invalidations_fi07_finally_without_return_preserves_try_catch() {
    let expr = resolve_alias("Fi07FinallyPreservesResult");
    assert_literal_union(&expr, &["from-try", "from-catch"]);
}

// ----- 8) `asserts x is T` on a dotted member path ----------------------
// TS7: the asserts predicate operates on `c.value` (the dotted path).
// After `fi08AssertNonNullable(c.value)`, `c.value` narrows to
// `NonNullable<string | undefined>` = `string`.
#[test]
#[ignore = "typeinfo currently does not narrow a dotted member path after asserts(this_path) is NonNullable; keep as the future Fi08 asserts-on-dotted-path contract"]
fn flow_invalidations_fi08_asserts_narrows_dotted_member_path() {
    let expr = resolve_alias("Fi08AssertDottedPathResult");
    assert_primitive(&expr, PrimitiveName::String);
}

// ----- 9) `assertNever`-style exhaustive tail joins handled cases -------
// TS7: a default branch that calls `f(value: never): never` contributes
// `never` to the return-type join. The function still returns the union
// of the handled cases: `string | number`.
#[test]
#[ignore = "typeinfo currently does not subtract the never-returning exhaustive default from the join while accumulating the handled case returns; keep as the future Fi09 exhaustive-tail contract"]
fn flow_invalidations_fi09_exhaustive_never_tail_does_not_widen_return() {
    let expr = resolve_alias("Fi09ExhaustiveResult");
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Number);
}
