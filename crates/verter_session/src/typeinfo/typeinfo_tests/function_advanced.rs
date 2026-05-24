//! @ai-generated - Advanced function-type contracts.
//!
//! TDD-red tests describing TS7 behaviour for declaration-level overloads,
//! `this` parameters, constructor types, generic function aliases,
//! call+construct hybrid interfaces, higher-order generic functions, and
//! `void`-return preservation.

use super::support::*;
use verter_type_expr::ObjectMember;

const FUNCTION_ADVANCED: &str = include_str!("fixtures/function_advanced.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/function_advanced.ts", FUNCTION_ADVANCED);
}

#[test]
#[ignore = "typeinfo currently does not pick the matching overload from a string-literal call site; keep as the future overload-selection contract"]
fn function_advanced_overload_call_picks_matching_signature_return() {
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
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "LookupCountResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not return the LAST overload's return type from `ReturnType<typeof f>` over an overloaded function; keep as the future overload-ReturnType contract"]
fn function_advanced_return_type_of_overloaded_function_uses_last_overload() {
    // TS7 quirk: `ReturnType<typeof lookup>` on an overloaded function
    // returns the LAST visible overload's return type — here the third
    // overload returns `boolean`. The implementation signature is NOT
    // considered. Active discriminator: the result is `boolean`, not the
    // union of all overloads or `string | number | boolean | undefined`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "LookupReturnType",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

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

#[test]
#[ignore = "typeinfo currently does not project `ThisParameterType<T>` to the declared `this` annotation; keep as the future this-parameter projection contract"]
fn function_advanced_this_parameter_type_returns_this_annotation() {
    // TS7 contract: `ThisParameterType<typeof withReceiver>` = the type of
    // the `this` parameter annotation = `{ value: number }`.
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

#[test]
#[ignore = "typeinfo currently does not strip the `this` parameter via `OmitThisParameter<T>`; keep as the future OmitThisParameter contract"]
fn function_advanced_omit_this_parameter_returns_function_without_this() {
    // TS7 contract: `OmitThisParameter<typeof withReceiver>` =
    // `(factor: number) => number`. The function type is republished with
    // the `this` slot stripped.
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

#[test]
#[ignore = "typeinfo currently does not reduce `ConstructorParameters<T>` to the constructor's parameter tuple; keep as the future ConstructorParameters contract"]
fn function_advanced_constructor_parameters_publishes_constructor_arg_tuple() {
    // TS7 contract: `ConstructorParameters<Ctor>` for
    // `new (id: string) => { id: string; ready: boolean }` = `[id: string]`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "CtorParams",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `InstanceType<Ctor>` to the constructor's return shape; keep as the future InstanceType contract"]
fn function_advanced_instance_type_publishes_constructor_return_shape() {
    // TS7 contract: `InstanceType<Ctor>` for
    // `new (id: string) => { id: string; ready: boolean }` =
    // `{ id: string; ready: boolean }`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "CtorInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id", "ready"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["ready"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

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

#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<T>` on a call+construct interface to the CALL signature parameters; keep as the future hybrid-call-parameters contract"]
fn function_advanced_call_construct_hybrid_parameters_uses_call_signature() {
    // TS7 contract: For an interface declaring both a call signature and a
    // construct signature, `Parameters<typeof x>` reduces against the CALL
    // signature. Here `Callable` has `(a: number): string` as the call
    // signature, so `Parameters<typeof callable>` = `[a: number]`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "CallableCallParams",
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

#[test]
#[ignore = "typeinfo currently does not reduce `ReturnType<T>` on a call+construct interface to the CALL signature return; keep as the future hybrid-call-return contract"]
fn function_advanced_call_construct_hybrid_return_type_uses_call_signature() {
    // TS7 contract: `ReturnType<typeof callable>` reduces against the CALL
    // signature `(a: number): string` = `string`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "CallableCallReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `ConstructorParameters<T>` on a call+construct interface to the CONSTRUCT signature parameters; keep as the future hybrid-construct-parameters contract"]
fn function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature() {
    // TS7 contract: `ConstructorParameters<typeof callable>` reduces against
    // the CONSTRUCT signature `new (b: string): { value: number }`. Result:
    // `[b: string]`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "CallableCtorParams",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `InstanceType<T>` on a call+construct interface to the CONSTRUCT signature return; keep as the future hybrid-construct-return contract"]
fn function_advanced_call_construct_hybrid_instance_type_uses_construct_signature() {
    // TS7 contract: `InstanceType<typeof callable>` = `{ value: number }` —
    // the return shape of the CONSTRUCT signature.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "CallableCtorInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["value"]);
    assert_primitive(&props["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

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
#[ignore = "typeinfo currently does not return the higher-order composition's concrete `(a: string) => boolean` for an explicit instantiation; keep as the future higher-order generic-return contract"]
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

#[test]
#[ignore = "typeinfo currently does not preserve the declared `void` return on a function-type alias; keep as the future void-return preservation contract"]
fn function_advanced_void_callback_return_preserves_void() {
    // TS7 contract: `type VoidCallback = () => void` declares a function
    // with `void` return. `ReturnType<typeof voidCallback>` = `void`. The
    // TS7 quirk is that void-return functions are assignable from functions
    // returning any value, but the DECLARED type is still `void` and what
    // `ReturnType<>` projects.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "VoidCallbackReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Void);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `ReturnType<typeof <Class>.prototype.<method>>` to the declared method return; keep as the future class-method-prototype extraction contract"]
fn function_advanced_class_method_prototype_extraction_projects_return() {
    // TS7 contract: `typeof MethodHolder.prototype.greet` is the canonical
    // way to extract a class method as a callable type. `ReturnType<>` over
    // that callable yields the method's declared return — here `string`.
    //
    // Verified via tsgo `IsExactly<ExtractedGreetReturn, string>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "ExtractedGreetReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce `Parameters<typeof <Class>.prototype.<method>>` to the method's parameter tuple; keep as the future class-method-parameter extraction contract"]
fn function_advanced_class_method_prototype_extraction_projects_parameters() {
    // TS7 contract: `Parameters<ExtractedGreetMethod>` =
    // `[name: string]` — a single-element labelled tuple matching the
    // method's declared `(name: string)` parameter list.
    //
    // Verified via tsgo `IsExactly<ExtractedGreetParams, [name: string]>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "ExtractedGreetParams",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not perform overload selection at a numeric call site against a generic-vs-string-specific overload pair; keep as the future overload-selection generic-binds-literal contract"]
fn function_advanced_overload_generic_first_binds_to_literal_argument() {
    // TS7 contract: `overloadedTypeParam(42 as 42)` against the overload set
    //   <T>(x: T): T
    //   (x: string): string
    // The number literal `42` is NOT assignable to `string`, so the
    // string-specific overload is filtered out. Only the generic overload
    // remains; it binds `T = 42` (no widening because of `as 42`), and the
    // return is the literal `42`.
    //
    // Verified via tsgo `IsExactly<OverloadedGenericResult, 42>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "OverloadedGenericResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 42.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not perform overload selection where the FIRST generic overload matches a non-const string argument and widens T to the primitive; keep as the future first-overload-wins generic-widening contract"]
fn function_advanced_overload_generic_first_widens_t_to_string_for_string_argument() {
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
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "OverloadedStringResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not perform constraint-aware generic inference from an `as const` literal argument; keep as the future constrained-generic-literal-inference contract"]
fn function_advanced_constrained_generic_infers_literal_under_as_const() {
    // TS7 contract: `constrainedIdentity<T extends string>(x: T): T` called
    // with `"constrained" as const`. The `as const` assertion locks the
    // argument's apparent type to the literal `"constrained"`; the
    // `T extends string` constraint is satisfied (literal types are
    // assignable to their widened primitive bound); `T` is inferred as the
    // literal `"constrained"` (NOT widened to `string`). The return is
    // therefore the literal `"constrained"`.
    //
    // Verified via tsgo `IsExactly<ConstrainedIdentityResult, "constrained">`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/function_advanced.ts",
        "ConstrainedIdentityResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "constrained");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
