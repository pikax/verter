//! @ai-generated - Call-site resolution contracts.
//!
//! Sibling of `function_advanced.rs`. Where that file pins the SHAPES of
//! function and constructor TYPES (call signature, this-annotation, hybrid
//! call+construct interface, generic function alias), THIS file pins the
//! resolver's CALL-SITE behaviour: overload picking against concrete argument
//! lists, generic inference from positional arguments and callback signatures,
//! `this`-receiver binding, extracted-method invocation via
//! `Function.prototype.call`, and constructor overload selection.
//!
//! Each scenario funnels its call through a synthetic wrapper function so the
//! probe surface is uniformly `ReturnType<typeof wrapper>`. The TS7 emission
//! for every aliased type below was empirically confirmed via
//! `IsExactly<actual, expected>` assertions compiled with TypeScript.
//!
//! Ten rows are `#[oracle_row]` lifts: they are seated in the
//! `ORACLE_QUERY_SPECS` registry and compare Verter's `Expanded` projection
//! against a checked-in tsgo snapshot through the shared driver. Two rows still
//! fail and name their real blocker — contextual-callback overload selection and
//! the extracted `Function.prototype.call` return. The abstract-constructor
//! `InstanceType<>` row resolves correctly but is owned by U2.CLASS_SURFACES and
//! has no oracle seat, so it stays ignored; its reason says so.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

const CALL_RESOLUTION: &str = include_str!("fixtures/call_resolution.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/call_resolution.ts", CALL_RESOLUTION);
}

// ---------------------------------------------------------------------------
// (1) Overload selection — contextual callback return picks the matching arm
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not pick the overload whose callback return type matches the contextual literal; keep as the future contextual-callback overload-selection contract"]
fn call_resolution_contextual_callback_return_picks_first_overload() {
    // TS7 contract: `pick("hello", (v) => "ok")` against the overload set
    //   <T extends string>(value: T, cb: (v: T) => "ok"): T
    //   <T extends number>(value: T, cb: (v: T) => "nope"): T
    // The first overload matches: `value = "hello"` binds `T = "hello"`, the
    // callback's contextual return type is `"ok"`, and the inline `return
    // "ok"` is contextually typed to that literal. The second overload is
    // filtered out because `"hello"` is not assignable to `number`. The
    // call's return is the literal `"hello"`.
    //
    // Verified via tsc `IsExactly<ContextualPickResult, "hello">`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/call_resolution.ts",
        "ContextualPickResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "hello");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// (2) Overload selection — optional / rest parameter ordering
// ---------------------------------------------------------------------------

// TS7 contract: `call("x")` against
//   (a: string): "with-a"
//   (a: string, b?: number): "with-b"
//   (...rest: string[]): "rest"
// All three overloads are applicable to a 1-arg call (the second's `b`
// is optional; the third's rest accepts one element). TS picks the FIRST
// matching overload in declaration order. Result: `"with-a"`.
//
// Verified via tsc `IsExactly<CallOptional1Result, "with-a">`.
#[oracle_row]
#[test]
fn call_resolution_optional_overload_picks_first_arity_matching_signature() {}

// TS7 contract: `call("x", 1)` against the same overload set as above.
// The first overload `(a: string)` does NOT accept two args; the second
// `(a: string, b?: number)` does; the third `(...rest: string[])` does
// NOT accept `number` for its element type. Only the second matches.
// Result: `"with-b"`.
//
// Verified via tsc `IsExactly<CallOptional2Result, "with-b">`.
#[oracle_row]
#[test]
fn call_resolution_optional_overload_picks_two_arg_signature_when_required() {}

// TS7 contract: `call("x", "y", "z")` against the same overload set. The
// first two overloads do NOT accept three args (the second's optional
// slot is a single `number?`, not a rest). Only the third
// `(...rest: string[])` accepts the variadic string list. Result:
// `"rest"`.
//
// Verified via tsc `IsExactly<CallOptional3Result, "rest">`.
#[oracle_row]
#[test]
fn call_resolution_rest_overload_picks_rest_signature_when_required() {}

// ---------------------------------------------------------------------------
// (3) Union argument does NOT distribute through overloads
// ---------------------------------------------------------------------------

// TS7 contract — empirically verified: a union argument does NOT
// distribute through a set of literal-key overloads. The call
// `lookup(key: "a" | "b")` against the overload set
//   (key: "a"): "value-a"
//   (key: "b"): "value-b"
//   (key: "a" | "b"): "value-a" | "value-b"
// selects the THIRD overload (the only one whose parameter type is a
// supertype of `"a" | "b"`). Without that union overload, TS reports
// "No overload matches this call". Result: `"value-a" | "value-b"`.
//
// Verified via tsc `IsExactly<LookupUnionResult, "value-a" | "value-b">`.
#[oracle_row]
#[test]
fn call_resolution_union_argument_picks_union_compatible_overload() {}

// TS7 contract: `lookup("a")` against
//   (key: "a"): "value-a"
//   (key: "b"): "value-b"
//   (key: "a" | "b"): "value-a" | "value-b"
// The first overload matches first in declaration order; TS picks it
// and the call's return is the literal `"value-a"` (NOT the union from
// the third overload, even though that third overload would also
// accept the call).
//
// Verified via tsc `IsExactly<LookupSpecificAResult, "value-a">`.
#[oracle_row]
#[test]
fn call_resolution_specific_literal_argument_picks_matching_overload_first() {}

// TS7 contract: `lookup("b")` against the same set above. The first
// overload `(key: "a"): "value-a"` does NOT accept `"b"` and is
// skipped. The second overload `(key: "b"): "value-b"` matches and is
// picked before the third union-accepting overload. The call's return
// is the literal `"value-b"`.
//
// Verified via tsc `IsExactly<LookupSpecificBResult, "value-b">`.
#[oracle_row]
#[test]
fn call_resolution_specific_literal_argument_skips_non_matching_first_overload() {}

// ---------------------------------------------------------------------------
// (4) Generic inference from a positional argument with callback contextual
//     binding
// ---------------------------------------------------------------------------

// TS7 contract: `withCallback<T>(cb: (item: T) => unknown, item: T): T`
// called with `((item) => item, "literal" as const)`. TS infers `T` from
// the SECOND positional argument (`item: T`); the `as const` locks the
// argument type to the literal `"literal"`. The callback's parameter
// type `item: T` is bound contextually to `"literal"` from that
// inference, and the callback's return is `unknown` (it does not
// constrain `T`). The call's return is the literal `"literal"`.
//
// Verified via tsc `IsExactly<CallbackParamInfer, "literal">`.
#[oracle_row]
#[test]
fn call_resolution_generic_infers_from_positional_argument_through_callback_signature() {}

// ---------------------------------------------------------------------------
// (5) Generic inference from a callback's return type
// ---------------------------------------------------------------------------

// TS7 contract: `lift<T>(cb: () => T): { value: T }` called with
// `() => 42 as const`. The callback returns the literal `42`; `T` is
// inferred from that return. The call's return is the structural shape
// `{ value: 42 }`.
//
// Verified via tsc `IsExactly<CallbackReturnInfer, { value: 42 }>`.
#[oracle_row]
#[test]
fn call_resolution_generic_infers_from_callback_return_type() {}

// ---------------------------------------------------------------------------
// (6) Generic inference from an object literal argument
// ---------------------------------------------------------------------------

// TS7 contract: `configure<T extends { mode: string }>(config: T): T`
// called with `{ mode: "active", debug: true }`. TS infers `T` as the
// INFERRED shape of the literal — INCLUDING the excess `debug` property
// — but widens individual property types under standard inference
// (no `as const`):
//   - `mode: "active"` widens to `string`
//   - `debug: true` widens to `boolean`
// Result: `{ mode: string; debug: boolean }`.
//
// Verified via tsc `IsExactly<ObjectLiteralInfer, { mode: string; debug:
// boolean }>`.
#[oracle_row]
#[test]
fn call_resolution_generic_infers_object_literal_including_excess_properties() {}

// ---------------------------------------------------------------------------
// (7) `this`-receiver call binds via ordinary method access
// ---------------------------------------------------------------------------

// TS7 contract: `receiverObj.greet("!")` on
//   greet(this: { data: string }, suffix: string): string
// The `this` parameter is bound by ordinary method access on
// `receiverObj`; it is invisible to `Parameters<>` (see
// `function_advanced_parameters_omits_this_slot`). The call's return is
// the declared `string`.
//
// Verified via tsc `IsExactly<ThisReceiverResult, string>`.
#[oracle_row]
#[test]
fn call_resolution_this_receiver_method_call_returns_declared_return() {}

// ---------------------------------------------------------------------------
// (8) Method extracted via prototype, invoked through `.call()`
// ---------------------------------------------------------------------------

// TS7 contract: `Greeter.prototype.greet` extracts the method as a
// callable. Invoking it through `.call(new Greeter(), "test")` binds the
// receiver and the argument; the call's return is the declared `string`.
//
// Verified via tsc `IsExactly<ExtractedMethodResult, string>`.
#[oracle_row]
#[test]
fn call_resolution_extracted_prototype_method_call_returns_declared_return() {}

// ---------------------------------------------------------------------------
// (9) Constructor overloads — `ConstructorParameters<typeof Class>` picks the
//     last overload
// ---------------------------------------------------------------------------

#[test]
fn call_resolution_constructor_parameters_uses_last_overload() {
    // TS7 contract — mirrors `ReturnType<typeof f>` on an overloaded
    // function: `ConstructorParameters<typeof CtorOverloaded>` returns the
    // LAST visible overload's parameter tuple. The class declares
    //   constructor(value: string);
    //   constructor(value: number, multiplier: number);
    // and `ConstructorParameters<>` reduces against the LAST overload:
    // `[value: number, multiplier: number]`.
    //
    // Verified via tsc
    // `IsExactly<CtorParams1, [value: number, multiplier: number]>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/call_resolution.ts",
        "CtorParams1",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 2);
    assert_primitive(&elements[0].ty, PrimitiveName::Number);
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// (10) Abstract constructor type — `InstanceType<>` reduces to the class shape
// ---------------------------------------------------------------------------

#[test]
#[ignore = "`InstanceType<abstract new (name: string) => AbstractBase>` reduces to the abstract class instance shape (`name: string` plus `describe(): string`) and the row PASSES under --include-ignored; it stays ignored because it has no `ORACLE_QUERY_SPECS` seat: `ProofRequirement::Ts7Oracle` requires a registry entry, a vendored source, a checked-in tsgo snapshot, and retained lift-migration provenance from the audited lift command. Lift under U2.CLASS_SURFACES when the row is seated"]
fn call_resolution_abstract_constructor_instance_type_projects_class_shape() {
    // TS7 contract: `InstanceType<abstract new (name: string) => AbstractBase>`
    // reduces to `AbstractBase` (an abstract class). The instance shape
    // includes:
    //   - `name: string` (synthesised from the `public name: string`
    //     parameter property)
    //   - `describe(): string` (the declared abstract method)
    //
    // `abstract new (...)` is a valid constructor type in TS; `InstanceType<>`
    // accepts both `new (...)` and `abstract new (...)` and projects the
    // return shape uniformly.
    //
    // Verified via tsc `IsExactly<AbstractInstanceShape, AbstractBase>` and
    // direct member-access probes (`instance.name: string`,
    // `instance.describe: () => string`).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/call_resolution.ts",
        "AbstractInstanceShape",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let names = prop_names(&props);
    assert!(
        names.contains(&"name"),
        "abstract instance shape must publish `name`; got {names:?}"
    );
    assert!(
        names.contains(&"describe"),
        "abstract instance shape must publish `describe`; got {names:?}"
    );
    assert_primitive(&props["name"].ty, PrimitiveName::String);
    // `describe` is a method; support helper converts it to a Function
    // ObjectProperty. Inspect its return.
    let describe = function_type(&props["describe"].ty);
    assert!(
        describe.parameters.is_empty(),
        "describe() must take no parameters; got {:?}",
        describe.parameters
    );
    assert_primitive(
        describe
            .return_type
            .as_ref()
            .expect("describe() must declare a return type"),
        PrimitiveName::String,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// (11) `this`-receiver call inside a class method body
// ---------------------------------------------------------------------------

#[test]
#[ignore = "port gap: the flow lane does not model `new` construction calls (the classifier's UnmodeledCall arm — corpus debts D10/D11) nor the `this` receiver in class method bodies (`this` is not modeled — the receiver capability is separate work, see flow_return_substrate.rs). The demand path: wrapper body -> `instance.run()` frame-rooted member call -> the local `instance` binding's `new` initializer is unmodeled, so the member projection misses and the position fails closed with `unmodeledPosition`. tsgo pins `string`."]
fn call_resolution_class_this_member_call_returns_declared_return() {
    // TS7 contract: `class ThisMemberCaller { helper(): string; run() { return
    // this.helper() } }`. `run` carries no return annotation, so its return is
    // body-derived: the `this.helper()` call's receiver is the class instance
    // and the call resolves `helper`'s declared `string`. A wrapper calls
    // `new ThisMemberCaller().run()`, so the probe surface is the wrapper's
    // inferred return.
    //
    // Verified via tsc `IsExactly<ClassThisMemberResult, string>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/call_resolution.ts",
        "ClassThisMemberResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
