//! @ai-generated - Modern TypeScript feature contracts beyond `NoInfer<T>`
//! and `<const T>`, both of which are already covered in `no_infer.rs` and
//! `const_type_param.rs`.
//!
//! Each test characterises the TS7 emission for the corresponding feature:
//!
//!   * Variance annotations `<in T>` / `<out U>` / `<in out V>` — variance
//!     is enforced by assignability, not by the structural surface. We
//!     assert the projected interface members survive type-argument
//!     application. (Verter handles.)
//!   * `using` declarations + `Symbol.dispose` — characterised against a
//!     SIMULATED structural shape (DisposableLike + a normal try/finally
//!     helper) that yields the same return-type surface as the real
//!     `using` form would. A companion test exercises the real `using`
//!     keyword directly. See fixture comments for details. (Verter
//!     handles both the simulated form and the real keyword.)
//!   * `await using` + `Symbol.asyncDispose` — same hermeticity workaround
//!     as `using`. (Verter emits `semanticMiss` for the async + try/finally
//!     body; `#[ignore]`d as a future contract.)
//!   * Import attributes — characterised against an `as const` simulated
//!     form because the real `with { type: "json" }` syntax requires a real
//!     on-disk module. (Verter does NOT surface readonly + does NOT reduce
//!     the indexed access; `#[ignore]`d as future contracts.)
//!   * `satisfies` operator deep behaviour — literal keys preserved by
//!     inference (Verter handles); inner values widen unless `as const`
//!     is applied (Verter currently preserves the literal — TS7 widens to
//!     the primitive; `#[ignore]`d as a future contract).

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

const MODERN_TS_FEATURES: &str = include_str!("fixtures/modern_ts_features.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/modern_ts_features.ts", MODERN_TS_FEATURES);
}

// ---------------------------------------------------------------------------
// Variance annotations — covariant / contravariant / invariant
// ---------------------------------------------------------------------------

#[test]
fn variance_annotation_out_projects_covariant_interface_members() {
    // TS7 contract: `Producer<string>` projects to an object surface with the
    // covariant slot `create(): string`. The `out` variance annotation does
    // not change the structural shape — it only restricts assignability.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "ProducerString",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["create"]);
    let create = function_type(&props["create"].ty);
    assert_eq!(create.parameters.len(), 0);
    let ret = create
        .return_type
        .as_ref()
        .expect("Producer<string>.create must have a return type");
    assert_primitive(ret, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn variance_annotation_in_projects_contravariant_interface_members() {
    // TS7 contract: `Consumer<number>` projects to an object surface with the
    // contravariant slot `consume(value: number): void`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "ConsumerNumber",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["consume"]);
    let consume = function_type(&props["consume"].ty);
    assert_eq!(consume.parameters.len(), 1);
    assert_primitive(&consume.parameters[0].ty, PrimitiveName::Number);
    let ret = consume
        .return_type
        .as_ref()
        .expect("Consumer<number>.consume must have a return type");
    assert_primitive(ret, PrimitiveName::Void);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn variance_annotation_in_out_projects_invariant_interface_members() {
    // TS7 contract: `Invariant<boolean>` projects to an object surface with
    // the invariant slot `transfer(value: boolean): boolean`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "InvariantBoolean",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["transfer"]);
    let transfer = function_type(&props["transfer"].ty);
    assert_eq!(transfer.parameters.len(), 1);
    assert_primitive(&transfer.parameters[0].ty, PrimitiveName::Boolean);
    let ret = transfer
        .return_type
        .as_ref()
        .expect("Invariant<boolean>.transfer must have a return type");
    assert_primitive(ret, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// `using` declaration — SIMULATED via DisposableLike + try/finally
// ---------------------------------------------------------------------------

#[test]
fn using_declaration_simulated_return_type_resolves_to_primitive() {
    // TS7 contract (simulated): `consumeDisposable()` returns `string`. The
    // SIMULATED form characterises the surface a literal `using` form would
    // produce — `using resource = makeDisposable(); return resource.value;`
    // — without depending on lib.esnext.disposable being in scope.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "ConsumeDisposableResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// `await using` — SIMULATED via AsyncDisposableLike + try/finally
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently emits a `semanticMiss` for Awaited<ReturnType<typeof async_helper>> where the async helper has a try/finally body and an explicit Promise<T> annotation; keep as the future `await using`-equivalent return-type contract"]
fn await_using_simulated_return_type_resolves_to_primitive() {
    // TS7 contract (simulated): `Awaited<ReturnType<typeof consumeAsyncDisposable>>`
    // unwraps the Promise<number> to `number`. The SIMULATED form characterises
    // the surface a literal `await using` form would produce.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "AsyncConsumeResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Import attributes — SIMULATED via `as const` literal
// ---------------------------------------------------------------------------

#[test]
#[ignore = "MODULE_AUGMENTATION reducer complete: Verter resolves `typeof importedJsonConfig` to the readonly `as const` object `{ readonly name: \"verter-fixture\"; readonly version: 1 }` (verified). NOT oracle-liftable — the `as const` value root is gate-rejected at the oracle source-walk (oracle admission Reject(ConstAssertion)); lift pending a const-assertion source-walk carve-out"]
fn import_attribute_simulated_resolves_imported_json_shape() {
    // TS7 contract (simulated): `typeof importedJsonConfig` where
    // `importedJsonConfig` is `{ name: "verter-fixture", version: 1 } as const`
    // projects to a readonly object whose members are string and number
    // literals. The real `import data from "./config.json" with { type: "json" }`
    // form would yield an identical surface; tsgo cannot resolve the JSON
    // module without an on-disk file.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "ImportedJsonConfig",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["name", "version"]);
    assert!(props["name"].readonly);
    assert!(props["version"].readonly);
    assert_string_literal(&props["name"].ty, "verter-fixture");
    assert_number_literal(&props["version"].ty, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `ImportedJsonConfig["name"]` reduces the string-literal index chain
// over the `as const` object alias to the literal `"verter-fixture"`. The
// lifted body is the registry-keyed `oracle::run_row` shared-driver call
// comparing Verter's `Expanded` projection against the checked-in tsgo
// snapshot. Trace re-homes the row to `U2.INDEXED_ACCESS`.
#[oracle_row]
#[test]
fn import_attribute_simulated_string_literal_indexed_member() {}

// ---------------------------------------------------------------------------
// `satisfies` operator deep behaviour
// ---------------------------------------------------------------------------

#[test]
fn satisfies_preserves_literal_keys_under_keyof_typeof() {
    // TS7 contract: `keyof typeof cfg` where
    // `cfg = { a: { count: 1 }, b: { count: 2 } } satisfies CfgShape`
    // preserves the literal keys "a" | "b" — `satisfies` constrains the value
    // against `CfgShape` (Record<string, CfgEntry>) WITHOUT widening the
    // value's type. The inferred type of `cfg` keeps its literal-keyed shape.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "CfgKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["a", "b"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves the inner literal `1` instead of widening to `number` under `satisfies` (without `as const`); TS7 widens to the primitive — keep as the future satisfies-widen-value contract"]
fn satisfies_widens_inner_value_to_primitive_without_as_const() {
    // TS7 contract: `typeof cfg.a.count` where `cfg` is `satisfies CfgShape`
    // resolves to the PRIMITIVE `number` — NOT the literal `1`. `satisfies`
    // does not perform an `as const` narrowing; inner value positions widen
    // to their declared primitive shape. This is the documented `satisfies`
    // != `as const` distinction.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "CfgValueACount",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Edge cases — variance substitution, satisfies array literal, real `using`
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently emits a `semanticMiss` for `Parameters<NumberConsumer[\"consume\"]>` — substituting `T = number` from a variance-annotated `<in T>` parameter into a method's parameter type through `Parameters<...>` is not yet reduced; keep as the future variance-substitution-through-method contract"]
fn variance_annotation_in_substitution_through_consumer_consume_parameters() {
    // TS7 contract: `Parameters<NumberConsumer["consume"]>` =
    // `[value: number]` — a single-element labelled tuple whose element type
    // is the substituted `T = number`. The variance-annotated `<in T>`
    // parameter on `Consumer` participates in normal type substitution; the
    // `in` annotation only restricts assignability and does NOT block T from
    // entering the consume method's parameter type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "NumberConsumerParameters",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!(
            "expected Parameters<NumberConsumer[\"consume\"]> to project to a tuple, got {expr:?}"
        );
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not widen the inferred type of a `satisfies readonly number[]` array literal to `number[]`; keep as the future satisfies-array-widen contract"]
fn satisfies_array_literal_widens_to_primitive_array() {
    // TS7 contract: `typeof arrSat` where
    // `arrSat = [1, 2, 3] satisfies readonly number[]` resolves to
    // `number[]` — the array literal is widened to the primitive array
    // shape because `satisfies` does NOT preserve the tuple shape, and the
    // value is not `as const`-narrowed. This is the documented `satisfies`
    // != `as const` distinction applied to array literals.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "ArrSatType",
        &[],
        ProjectionMode::Expanded,
    );

    assert_array_of_primitive(&expr, PrimitiveName::Number);
    let TypeExpr::Array { readonly, .. } = &expr else {
        panic!("expected ArrSatType to project to an Array, got {expr:?}");
    };
    assert!(
        !readonly,
        "`satisfies readonly number[]` without `as const` must NOT mark the inferred array readonly"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn using_declaration_real_keyword_return_type_resolves_to_primitive() {
    // TS7 contract: `ReturnType<typeof consumeDisposableUsing>` where
    // `consumeDisposableUsing` declares `using resource = makeDisposable();
    // return resource.value;` resolves to `string`. This is the real `using`
    // keyword form; the simulated `consumeDisposable` form above
    // characterises the same return surface via a try/finally body.
    //
    // The pair locks in that the real-keyword form and the simulated form
    // converge on the same return-type surface; the body-level difference
    // between `using` and try/finally is a parser-level concern this test
    // does not characterise.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/modern_ts_features.ts",
        "RealUsingResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
