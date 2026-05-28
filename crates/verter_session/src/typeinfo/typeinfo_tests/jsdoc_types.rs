//! JSDoc `{Type}` payloads are FIRST-CLASS regular types (U2-1).
//!
//! The owner directive (CLAUDE.md "typeinfo: JSDoc types are regular types"):
//! a JSDoc `{Type}` payload (`@type` / `@param` / `@returns`) lowers to a
//! `TypeExpr` exactly like `lower_ts_type` does for a TS annotation, and then
//! resolves through typeinfo's NORMAL five-mode dispatch — there is NO separate
//! JSDoc-type resolution path. These tests prove that a JSDoc-typed JS value
//! resolves through the shared resolver IDENTICALLY to the equivalent TS
//! annotation (the headline) and that an unknown JSDoc type misses the same way
//! a TS reference to an undeclared type would (the negative).
//!
//! Each test is discriminating: it FAILS against the pre-U2 tree (shallow
//! analysis ignored JSDoc `@type`, so `typeof x` resolved to the
//! initializer-inferred / `any` shape) and PASSES post-fix (the JSDoc payload
//! flows through the same `ValueDeclInfo.type_annotation` carrier a TS
//! annotation uses).

use super::support::*;

/// Two declarations of the same shape: one TS-annotated, one JSDoc-`@type`'d.
/// `Foo` is a real interface. The JSDoc-typed value's initializer is a bare
/// untyped object that does NOT structurally match `Foo` (it has a single
/// `decoy` member), so an implementation that ignored JSDoc and inferred from
/// the initializer would NOT produce `Foo`'s `{ a, b }` surface.
const JSDOC_TYPE_FIXTURE: &str = r#"
export interface Foo {
  a: string;
  b: number;
}

/** Annotated the TS way. */
export const tsTyped: Foo = { a: "x", b: 1 };

/** @type {Foo} */
export const jsdocTyped = { decoy: true };
"#;

#[test]
fn jsdoc_type_resolves_through_shared_resolver_identically_to_ts_annotation() {
    // Headline: `typeof jsdocTyped` (a JSDoc `@type {Foo}` value) must resolve
    // through the NORMAL dispatch to the SAME surface as `typeof tsTyped` (a TS
    // `: Foo` value). Proves the JSDoc payload is an ordinary type, not a
    // separate-path special case.
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/jsdoc-type.ts", JSDOC_TYPE_FIXTURE);

    let (ts_expr, ts_record) = evaluate_expr(
        &host,
        "/fixtures/jsdoc-type.ts",
        "typeof tsTyped",
        ProjectionMode::Expanded,
    );
    let (jsdoc_expr, jsdoc_record) = evaluate_expr(
        &host,
        "/fixtures/jsdoc-type.ts",
        "typeof jsdocTyped",
        ProjectionMode::Expanded,
    );

    // Both resolve to `Foo`'s object surface `{ a: string; b: number }`.
    let ts_props = object_props(&ts_expr);
    assert_eq!(
        prop_names(&ts_props),
        vec!["a", "b"],
        "TS-annotated `typeof tsTyped` must resolve to Foo's surface"
    );
    assert_primitive(&ts_props["a"].ty, PrimitiveName::String);
    assert_primitive(&ts_props["b"].ty, PrimitiveName::Number);

    let jsdoc_props = object_props(&jsdoc_expr);
    assert_eq!(
        prop_names(&jsdoc_props),
        vec!["a", "b"],
        "JSDoc `@type {{Foo}}` `typeof jsdocTyped` must resolve to Foo's surface through the \
         NORMAL resolver — NOT the initializer-inferred `{{ decoy: boolean }}` shape (that \
         would prove JSDoc was ignored / handled by a separate path)"
    );
    assert_primitive(&jsdoc_props["a"].ty, PrimitiveName::String);
    assert_primitive(&jsdoc_props["b"].ty, PrimitiveName::Number);

    // The JSDoc-typed value resolves to the EXACT SAME TypeExpr as the
    // TS-typed value — the strongest "no separate path" assertion.
    assert_eq!(
        jsdoc_expr, ts_expr,
        "a JSDoc `@type {{Foo}}` value must resolve to the identical TypeExpr a TS `: Foo` value \
         does (one shared resolver, one lowering)"
    );

    // NEGATIVE: the initializer's `decoy` member must NOT leak onto the surface
    // (it would if the implementation fell back to initializer inference).
    assert!(
        !jsdoc_props.contains_key("decoy"),
        "the JSDoc `@type` annotation must SHADOW the initializer shape; `decoy` must be absent"
    );

    // Both requests route through the shared TypeResolution dispatch.
    assert_query_mode(&ts_record, ProjectionModeTag::Expanded);
    assert_query_mode(&jsdoc_record, ProjectionModeTag::Expanded);
}

/// An unknown JSDoc `{Missing}` must miss the SAME way a TS `: Missing`
/// reference to an undeclared type misses — not via a JSDoc-specific path.
const JSDOC_UNKNOWN_FIXTURE: &str = r#"
/** @type {Missing} */
export const jsdocMissing = 0;

export const tsMissing: Missing = 0;
"#;

#[test]
fn unknown_jsdoc_type_misses_identically_to_unknown_ts_reference() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/jsdoc-missing.ts", JSDOC_UNKNOWN_FIXTURE);

    let (ts_expr, _) = evaluate_expr(
        &host,
        "/fixtures/jsdoc-missing.ts",
        "typeof tsMissing",
        ProjectionMode::Expanded,
    );
    let (jsdoc_expr, _) = evaluate_expr(
        &host,
        "/fixtures/jsdoc-missing.ts",
        "typeof jsdocMissing",
        ProjectionMode::Expanded,
    );

    // Both an undeclared TS reference and an undeclared JSDoc reference resolve
    // to the SAME unresolved shape — the JSDoc path does NOT invent a different
    // miss. The exact unresolved representation is whatever the shared resolver
    // produces for an undeclared value annotation; the discriminating invariant
    // is that the two paths agree.
    assert_eq!(
        jsdoc_expr, ts_expr,
        "an unknown JSDoc `{{Missing}}` must miss IDENTICALLY to an unknown TS `: Missing` \
         (one shared resolver, one miss) — got jsdoc={jsdoc_expr:?} vs ts={ts_expr:?}"
    );
}
