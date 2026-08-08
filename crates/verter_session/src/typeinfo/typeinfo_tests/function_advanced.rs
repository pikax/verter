//! @ai-generated - Advanced function-type contracts.
//!
//! TDD-red tests describing TS7 behaviour for declaration-level overloads,
//! `this` parameters, constructor types, generic function aliases,
//! call+construct hybrid interfaces, higher-order generic functions, and
//! `void`-return preservation.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;
use verter_type_expr::ObjectMember;

const FUNCTION_ADVANCED: &str = include_str!("fixtures/function_advanced.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/function_advanced.ts", FUNCTION_ADVANCED);
}

// TS7 contract: `lookup("count")` matches the second overload
// `function lookup(key: "count"): number`. The call-site return is
// `number`. The implementation signature is invisible to overload
// resolution.
//
// Indirection note: the fixture exposes this via
//   `LookupCountResult = ReturnType<typeof callLookupCount>`
// where `callLookupCount = () => lookup("count")`. The test therefore
// exercises "ReturnType of an inferred-return wrapper over an overload
// call" — semantically equivalent to "ReturnType of the overload call
// directly", but routed through one inference layer.
#[oracle_row]
#[test]
fn function_advanced_overload_call_picks_matching_signature_return() {}

// LIFTED: `ReturnType<typeof lookup>` selects the LAST VISIBLE overload of the
// ordered declaration group (the bodied implementation signature is
// hidden) and projects its `boolean` return. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_return_type_of_overloaded_function_uses_last_overload() {}

#[test]
fn function_advanced_parameters_omits_this_slot() {
    // TS7 contract: `Parameters<typeof withReceiver>` for
    // `function withReceiver(this: { value: number }, factor: number): number`
    // yields `[factor: number]`. The `this` parameter is invisible to
    // `Parameters<>`; only the regular parameters survive.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "WithReceiverParams",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// TS7 contract: `ThisParameterType<typeof withReceiver>` = the type of
// the `this` parameter annotation = `{ value: number }`.
#[test]
#[ignore = "`ThisParameterType<typeof withReceiver>` projects the declared `this` annotation `{ value: number }` today and the row PASSES under --include-ignored; it stays ignored because it cannot be oracle-seated: the harness source-side admission gate rejects the row with `Reject(ThisTypeOrParam)`, so no checked-in tsgo snapshot can be generated for it. Lift when the admission allowlist covers `this`-typed declarations"]
fn function_advanced_this_parameter_type_returns_this_annotation() {
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "WithReceiverThis",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["value"]);
    assert_primitive(&props["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// TS7 contract: `OmitThisParameter<typeof withReceiver>` =
// `(factor: number) => number`. The function type is republished with
// the `this` slot stripped.
#[test]
#[ignore = "`OmitThisParameter<typeof withReceiver>` republishes `(factor: number) => number` with the `this` slot stripped today and the row PASSES under --include-ignored; it stays ignored because it cannot be oracle-seated: the harness source-side admission gate rejects the row with `Reject(ThisTypeOrParam)`, so no checked-in tsgo snapshot can be generated for it. Lift when the admission allowlist covers `this`-typed declarations"]
fn function_advanced_omit_this_parameter_returns_function_without_this() {
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "WithReceiverOmitThis",
        &[],
        ProjectionMode::Expanded,
    );

    let function = function_type(&expr);
    assert_eq!(function.parameters.len(), 1);
    assert_primitive(&function.parameters[0].ty, PrimitiveName::Number);
    assert_primitive(
        function
            .return_type
            .as_ref()
            .expect("OmitThisParameter must preserve the return annotation"),
        PrimitiveName::Number,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `ConstructorParameters<Ctor>` reduces the construct-signature alias to
// the labelled parameter tuple `[id: string]` via the construct bucket. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_constructor_parameters_publishes_constructor_arg_tuple() {}

// LIFTED: `InstanceType<Ctor>` materialises the construct signature's declared
// return object `{ id: string; ready: boolean }`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_instance_type_publishes_constructor_return_shape() {}

#[test]
fn function_advanced_generic_function_alias_instantiates_to_concrete_signature() {
    // TS7 contract: `Mapper<string, number>` for
    // `type Mapper<T, R> = (input: T) => R` = `(input: string) => number`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "StringToNumberMapper",
        &[],
        ProjectionMode::Expanded,
    );

    let function = function_type(&expr);
    assert_eq!(function.parameters.len(), 1);
    assert_primitive(&function.parameters[0].ty, PrimitiveName::String);
    assert_primitive(
        function
            .return_type
            .as_ref()
            .expect("Mapper must preserve the return annotation"),
        PrimitiveName::Number,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `Parameters<typeof callable>` picks the CALL bucket of the hybrid
// call+construct interface — `[a: number]`, never the construct params. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_call_construct_hybrid_parameters_uses_call_signature() {}

// LIFTED: `ReturnType<typeof callable>` picks the CALL bucket of the hybrid
// interface and projects `string`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_call_construct_hybrid_return_type_uses_call_signature() {}

// LIFTED: `ConstructorParameters<typeof callable>` picks the CONSTRUCT bucket of
// the hybrid interface — `[b: string]`, never the call params. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature() {}

// LIFTED: `InstanceType<typeof callable>` picks the CONSTRUCT bucket of the hybrid
// interface and materialises `{ value: number }`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_call_construct_hybrid_instance_type_uses_construct_signature() {}

#[test]
fn function_advanced_hybrid_interface_publishes_both_call_and_construct_signatures() {
    // TS7 contract: `interface Callable { (a: number): string; new (b: string):
    // { value: number } }` — the published type carries BOTH a call signature
    // and a construct signature as object members. This characterises the
    // interface itself (independent of `Parameters<>` / `ConstructorParameters<>`
    // reduction) by inspecting `ObjectMember::CallSignature` and
    // `ObjectMember::ConstructSignature` directly.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "Callable",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Object(object) = &expr else {
        panic!("expected object with hybrid signatures, got {expr:?}");
    };
    let call_sig = object
        .properties
        .iter()
        .find_map(|m| match m {
            ObjectMember::CallSignature(f) => Some(f),
            _ => None,
        })
        .expect("Callable must publish a call signature");
    assert_eq!(call_sig.parameters.len(), 1);
    assert_primitive(&call_sig.parameters[0].ty, PrimitiveName::Number);
    assert_primitive(
        call_sig
            .return_type
            .as_ref()
            .expect("call signature must preserve return"),
        PrimitiveName::String,
    );

    let construct_sig = object
        .properties
        .iter()
        .find_map(|m| match m {
            ObjectMember::ConstructSignature(f) => Some(f),
            _ => None,
        })
        .expect("Callable must publish a construct signature");
    assert_eq!(construct_sig.parameters.len(), 1);
    assert_primitive(&construct_sig.parameters[0].ty, PrimitiveName::String);
    let return_props = object_props(
        construct_sig
            .return_type
            .as_ref()
            .expect("construct signature must preserve return"),
    );
    assert_eq!(prop_names(&return_props), vec!["value"]);
    assert_primitive(&return_props["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "owned by U6.CONTEXTUAL_CALLBACK. The explicit `compose<string, number, boolean>` instantiation resolves to `Unknown(semanticMiss)` today, not the concrete `(a: string) => boolean`: the composed result's parameter type comes from contextually typing the two composed callbacks against the instantiated type parameters, which is contextual-callback inference rather than call-site overload dispatch. Lift when U6.CONTEXTUAL_CALLBACK lands that inference"]
fn function_advanced_higher_order_composition_returns_concrete_function() {
    // TS7 contract: `compose<string, number, boolean>(f, g)` returns
    // `(a: string) => boolean`. The wrapped call binds `A = string`,
    // `B = number`, `C = boolean`, and the visible return is the
    // outer function type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "ComposeStringNumberBooleanResult",
        &[],
        ProjectionMode::Expanded,
    );

    let function = function_type(&expr);
    assert_eq!(function.parameters.len(), 1);
    assert_primitive(&function.parameters[0].ty, PrimitiveName::String);
    assert_primitive(
        function
            .return_type
            .as_ref()
            .expect("compose result must preserve the return annotation"),
        PrimitiveName::Boolean,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// TS7 contract: `type VoidCallback = () => void` declares a function
// with `void` return. `ReturnType<typeof voidCallback>` = `void`. The
// TS7 quirk is that void-return functions are assignable from functions
// returning any value, but the DECLARED type is still `void` and what
// `ReturnType<>` projects.
#[oracle_row]
#[test]
fn function_advanced_void_callback_return_preserves_void() {}

// LIFTED: `ReturnType<typeof MethodHolder.prototype.greet>` hops the synthesized
// `.prototype` instance projection to the method and projects its
// declared `string` return. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_class_method_prototype_extraction_projects_return() {}

// LIFTED: `Parameters<typeof MethodHolder.prototype.greet>` hops `.prototype` to
// the method and projects the labelled parameter tuple `[name: string]`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn function_advanced_class_method_prototype_extraction_projects_parameters() {}

// TS7 contract: `overloadedTypeParam(42 as 42)` against the overload set
//   <T>(x: T): T
//   (x: string): string
// The number literal `42` is NOT assignable to `string`, so the
// string-specific overload is filtered out. Only the generic overload
// remains; it binds `T = 42` (no widening because of `as 42`), and the
// return is the literal `42`.
//
// Verified via tsgo `IsExactly<OverloadedGenericResult, 42>`.
#[oracle_row]
#[test]
fn function_advanced_overload_generic_first_binds_to_literal_argument() {}

// TS7 contract: `overloadedTypeParam("hello")` against
//   <T>(x: T): T          // declared first
//   (x: string): string   // declared second
// TS picks the FIRST matching overload in declaration order. The generic
// overload `<T>(x: T): T` matches `"hello"` immediately. Without `as
// const` on the argument, `T` widens from the literal `"hello"` to the
// primitive `string` during inference. The return is therefore `string`
// (NOT the literal `"hello"`, NOT routed through the second signature).
//
// The string-specific second signature is functionally UNREACHABLE for any
// non-const string argument because the generic-first ordering shadows
// it. This test characterises:
//   (a) overload picking order is declaration-first (NOT specificity-first
//       and NOT last-wins), and
//   (b) generic T widens from a non-const literal argument to the
//       primitive constraint.
//
// Verified via tsgo `IsExactly<OverloadedStringResult, string>` (and
// `IsExactly<OverloadedStringResult, "hello">` = `false`).
#[oracle_row]
#[test]
fn function_advanced_overload_generic_first_widens_t_to_string_for_string_argument() {}

// TS7 contract: `constrainedIdentity<T extends string>(x: T): T` called
// with `"constrained" as const`. The `as const` assertion locks the
// argument's apparent type to the literal `"constrained"`; the
// `T extends string` constraint is satisfied (literal types are
// assignable to their widened primitive bound); `T` is inferred as the
// literal `"constrained"` (NOT widened to `string`). The return is
// therefore the literal `"constrained"`.
//
// Verified via tsgo `IsExactly<ConstrainedIdentityResult, "constrained">`.
#[oracle_row]
#[test]
fn function_advanced_constrained_generic_infers_literal_under_as_const() {}
