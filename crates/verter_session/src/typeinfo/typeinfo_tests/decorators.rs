//! @ai-generated - Synthetic TS7 (TC39 stage 3) decorators typeinfo contracts.
//!
//! Decorator semantics (the contract the resolver must honour):
//!
//!   * Identity-shaped decorators (the decorator returns the original
//!     constructor / method / accessor target unchanged) leave the
//!     class's published instance type equal to the un-decorated form.
//!   * A method's inferred return type survives method decoration —
//!     `@bound tag(): "tag" { ... }` still publishes a method whose
//!     ReturnType is the literal `"tag"`.
//!   * A field-with-initializer decorator's return shape
//!     `(initial: Value) => Value` does NOT widen the field's declared
//!     type; `count: number` stays `number` after `@tracked`.
//!   * An accessor decorator over `accessor x: T` publishes a public
//!     property of declared type `T` on the instance side.
//!   * A decorator factory's call-site literal argument (`withTag("v1")`)
//!     is captured in the returned decorator's closure but does NOT
//!     propagate into the decorated class's instance shape — the
//!     returned decorator is structurally identity-shaped, so the
//!     class's instance type equals the bare class type.
//!   * Reading `ctx.metadata` inside a decorator does not change the
//!     decorated class's structural shape.
//!
//! Every assertion below encodes TS7's exact emission. Tests where
//! Verter already produces the TS7-correct projection are active;
//! tests where Verter still diverges carry an `#[ignore]` reason
//! describing the missing semantics. Un-ignore as Verter starts to
//! pass them.

use super::support::*;

const DECORATORS: &str = include_str!("fixtures/decorators.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/decorators.ts", DECORATORS);
}

#[test]
fn decorators_identity_class_decorator_preserves_instance_shape() {
    // TS7 contract: `@logged` is identity (`(ctor, _ctx) => ctor`). The
    // published `InstanceType<typeof LoggedItem>` therefore equals the bare
    // class instance shape:
    //   { id: string; label(): string }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "LoggedItemInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["id", "label"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    let label = function_type(&props["label"].ty);
    assert_primitive(
        label.return_type.as_ref().expect("label() return type"),
        PrimitiveName::String,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not preserve a decorated method's literal-declared ReturnType through indexed-access on the class instance; keep as the future method-decorator literal ReturnType contract"]
fn decorators_identity_method_decorator_preserves_return_inference() {
    // TS7 contract: `@bound` is identity for method decorators. The method
    // body `return "tag"` against the declared return type `"tag"` keeps
    // the literal return. `ReturnType<MethodHost["tag"]>` = `"tag"`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "MethodHostTagReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "tag");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn decorators_method_decorator_keeps_method_on_instance() {
    // TS7 contract: identity method decoration leaves the method on the
    // instance shape. `InstanceType<typeof MethodHost>` =
    //   { tag(): "tag" }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "MethodHostInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["tag"]);
    let tag = function_type(&props["tag"].ty);
    assert_string_literal(tag.return_type.as_ref().expect("tag() return type"), "tag");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn decorators_identity_field_decorator_preserves_field_type() {
    // TS7 contract: `@tracked` returns an initializer of shape
    // `(initial: Value) => Value`. The field's declared type
    // `count: number` survives unchanged. `InstanceType<typeof FieldHost>`
    // = `{ count: number }`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "FieldHostInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count"]);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project the `accessor` keyword's public surface through identity accessor decoration; keep as the future accessor-decorator identity contract"]
fn decorators_identity_accessor_decorator_publishes_public_property() {
    // TS7 contract: `accessor visible: string` produces a public property
    // of declared type `string` on the instance side. `@readonlyGet` is
    // identity. `InstanceType<typeof AccessorHost>` = `{ visible: string }`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "AccessorHostInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["visible"]);
    assert_primitive(&props["visible"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn decorators_factory_call_does_not_widen_class_shape() {
    // TS7 contract: `@withTag("v1")` produces an identity class decorator
    // — the factory's literal argument lives in the decorator's closure
    // only. The decorated class's instance shape equals the bare class
    // shape:
    //   { payload: string }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "FactoryDecoratedInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["payload"]);
    assert_primitive(&props["payload"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn decorators_metadata_reader_decorator_preserves_class_shape() {
    // TS7 contract (`Symbol.metadata`): reading `ctx.metadata`
    // inside a class decorator is a no-op on the class's structural
    // shape. The decorator returns the original constructor unchanged,
    // so `InstanceType<typeof MetadataAware>` =
    //   { ready: boolean; describe(): "ready" | "pending" }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "MetadataAwareInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["describe", "ready"]);
    assert_primitive(&props["ready"].ty, PrimitiveName::Boolean);
    let describe = function_type(&props["describe"].ty);
    let return_type = describe
        .return_type
        .as_ref()
        .expect("describe() return type");
    assert_literal_union(return_type, &["pending", "ready"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate ReturnType against a decorated class instance's literal-union method via indexed access; keep as the future decorated-method literal-union ReturnType contract"]
fn decorators_metadata_reader_describe_return_is_literal_union() {
    // TS7 contract: `ReturnType<MetadataAware["describe"]>` =
    //   `"ready" | "pending"`. The decorator does not interfere with the
    // method's declared return type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "MetadataAwareDescribeReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["pending", "ready"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn decorators_const_type_param_factory_does_not_widen_class_shape() {
    // TS7 contract: `withConstTag<const Tag extends string>(_tag: Tag)`
    // captures the call-site literal in `Tag`. The `const` modifier forces
    // `Tag = "v2"` (no widening). The returned class decorator's signature is
    // structurally identity — `(ctor: T, _ctx) => T` — so the literal `Tag`
    // captured in the factory's closure does NOT propagate into the decorated
    // class's instance shape. The published instance equals the bare class:
    //   { visible: string }
    //
    // The discrimination is: a hypothetical resolver that mistakenly
    // augmented the class shape with the captured `Tag` literal (e.g. as a
    // brand or as a phantom `tag: "v2"` field) would fail the
    // `prop_names == ["visible"]` assertion.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "ConstTaggedInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["visible"]);
    assert_primitive(&props["visible"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn decorators_method_decorator_with_add_initializer_preserves_instance_shape() {
    // TS7 contract: `methodWithInit` invokes
    // `ctx.addInitializer(function () { ... })` — a `ClassMethodDecoratorContext`
    // hook that schedules a per-instance runtime side-effect at construction
    // time. The hook has NO effect on the method's type. The decorator
    // otherwise returns the method unchanged, so the published instance shape
    // equals the bare class:
    //   { ping(): "pong" }
    //
    // The literal return `"pong" as const` plus the explicit-type assertion
    // discriminates between "method survives" and "decorator dropped the
    // method" or "method return was widened to `string`". The assertion on
    // `ping` being a function type also rejects any path where the method was
    // re-bound to a non-function value at the type level.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "InitDecoratedInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["ping"]);
    let ping = function_type(&props["ping"].ty);
    assert_string_literal(
        ping.return_type.as_ref().expect("ping() return type"),
        "pong",
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project the `accessor` keyword's public surface even when the decorator's return shape is the same `ClassAccessorDecoratorTarget` it received; keep as the future accessor-decorator-with-explicit-target-return contract"]
fn decorators_accessor_decorator_returning_same_target_publishes_public_property() {
    // TS7 contract: `trackedAccessor` receives the synthesised
    // `ClassAccessorDecoratorTarget<unknown, T>` for `accessor count: number`
    // and returns the same target type. Returning an explicitly-typed
    // `ClassAccessorDecoratorTarget` (rather than letting inference fall
    // through to identity) is the documented way to transform the behaviour
    // without changing the type. The accessor's public surface is
    //   { count: number }
    //
    // This complements the existing identity-accessor test
    // (`decorators_identity_accessor_decorator_publishes_public_property`):
    // there `@readonlyGet` returns `target` and is implicitly identity; here
    // the decorator explicitly types its return as
    // `ClassAccessorDecoratorTarget<unknown, T>`, which is the shape a
    // transformation decorator would use. Either way the published type
    // surface is the same. A resolver that only handled the bare-identity
    // case would miss the explicit-target case and emit nothing for `count`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/decorators.ts",
        "AccessorTransformedInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count"]);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
