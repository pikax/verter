//! @ai-generated - Synthetic TypeScript class typeinfo contracts.
//!
//! Covers abstract classes, static inheritance, `extends` + `implements`
//! composition, generic class hierarchies with substituted type
//! parameters, the `ConstructorParameters` utility over a subclass that
//! adds its own constructor, deep `InstanceType` of a generic subclass,
//! and a `protected` member referenced inside a subclass method body.
//!
//! Every assertion below encodes TS7's exact emission. Tests where
//! Verter already produces the TS7-correct projection are active;
//! tests where Verter still diverges carry an `#[ignore]` reason
//! describing the missing semantics. Un-ignore as Verter starts to
//! pass them.
//!
//! Class semantics in TypeScript:
//!   * `InstanceType<typeof Class>` projects the instance shape: own
//!     declared instance fields, methods (as method-signature surfaces),
//!     plus every inherited instance member from every base in the
//!     extends chain. Members declared in `implements` interfaces must
//!     also surface on the instance.
//!   * `typeof Class` is the constructor-side type: own static fields +
//!     inherited static fields from base classes, plus a construct
//!     signature shaped from the subclass's own constructor.
//!   * `ConstructorParameters<typeof Subclass>` reads the subclass's
//!     OWN constructor parameter list, NOT the base's.
//!   * Generic substitution: when a subclass extends `Base<Concrete>`,
//!     the instance's inherited fields are emitted with the type
//!     parameter substituted (`T` => `Concrete`).
//!   * `protected` members are visible inside subclass method bodies
//!     for inference purposes; the published `protected` field does
//!     NOT appear on the instance projection of subclasses (the
//!     `InstanceType` is the public surface).

use super::support::*;

const CLASS_FEATURES: &str = include_str!("fixtures/class_features.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/class_features.ts", CLASS_FEATURES);
}

#[test]
#[ignore = "typeinfo currently publishes only the subclass's own members on InstanceType<typeof Dog> and does not include the abstract base's `name: string` field; keep as the future abstract-class inherited-field contract"]
fn class_features_abstract_subclass_instance_includes_inherited_and_own_members() {
    // TS7 contract: `InstanceType<typeof Dog>` projects the instance shape of
    // the concrete subclass. The base `Animal` is abstract but still
    // contributes its concrete `name: string` instance field. The subclass
    // adds the concrete `sound()` method whose return type is inferred from
    // the body `return "woof" as const;` as the string literal `"woof"`.
    //
    // Published surface: { name: string; sound(): "woof" }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "DogInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["name", "sound"]);
    assert_primitive(&props["name"].ty, PrimitiveName::String);
    let sound = function_type(&props["sound"].ty);
    assert_string_literal(
        sound.return_type.as_ref().expect("sound() return type"),
        "woof",
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate ReturnType<Class[\"method\"]> through indexed access on a class instance and the `as const` literal return is widened away; keep as the future class-method literal ReturnType contract"]
fn class_features_dog_sound_return_type_is_literal_woof() {
    // TS7 contract: `ReturnType<Dog["sound"]>` = `"woof"`. The method body
    // returns `"woof" as const`, so the inferred return type is the literal,
    // and `ReturnType` projects that literal.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "DogSoundReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "woof");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project static-member access on `typeof Subclass` through the base class's static surface; keep as the future static-inheritance contract"]
fn class_features_static_inheritance_resolves_inherited_field_type() {
    // TS7 contract: `typeof StepCounter.initial` looks up the `initial`
    // static field on the constructor-side type of `StepCounter`. Since
    // `StepCounter` does not declare its own `initial`, the lookup falls
    // through to its base `BaseCounter`, which declares
    // `static initial: string`. So the published type is `string`.
    //
    // The field is `string` (not `number`) by design — a numeric `0`
    // initialiser would let an implementer pass this test via numeric-literal
    // widening on the initialiser side rather than by actually walking the
    // static chain to the declared annotation.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "StepCounterInitial",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not invoke ReturnType against a static method reached via static inheritance; keep as the future static-inheritance ReturnType contract"]
fn class_features_static_inheritance_resolves_inherited_method_return() {
    // TS7 contract: `ReturnType<typeof StepCounter.describe>` follows the
    // static-inheritance chain to `BaseCounter.describe(): string`, then
    // projects its return type `string`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "StepCounterDescribeReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not compose `extends` + `implements` into the InstanceType projection; keep as the future class-implements contract"]
fn class_features_extends_plus_implements_projects_union_of_members() {
    // TS7 contract: `InstanceType<typeof Greeter>` exposes the union of:
    //   * the inherited `name: string` field from base `Named`, and
    //   * the implemented `greet(): string` method from the
    //     `Greetable` interface (the class body returns `"hello"` which
    //     widens to the declared interface return type `string`).
    //
    // Published surface: { name: string; greet(): string }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "GreeterInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["greet", "name"]);
    assert_primitive(&props["name"].ty, PrimitiveName::String);
    let greet = function_type(&props["greet"].ty);
    assert_primitive(
        greet.return_type.as_ref().expect("greet() return type"),
        PrimitiveName::String,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not substitute generic type parameters along an `extends Base<Concrete>` chain into the InstanceType projection; keep as the future generic-class inheritance contract"]
fn class_features_generic_subclass_substitutes_type_parameter_on_inherited_field() {
    // TS7 contract: `StringBox extends Box<string>`. The base `Box<T>`
    // declares `value: T`. After substituting `T = string`, the inherited
    // field on the subclass instance is `value: string`. The subclass also
    // adds its own `suffix(): ".str"` (from `"..." as const`).
    //
    // Published surface: { value: string; suffix(): ".str" }
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "StringBoxInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["suffix", "value"]);
    assert_primitive(&props["value"].ty, PrimitiveName::String);
    let suffix = function_type(&props["suffix"].ty);
    assert_string_literal(
        suffix.return_type.as_ref().expect("suffix() return type"),
        ".str",
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_constructor_parameters_uses_subclass_ctor_not_base_ctor() {
    // TS7 contract: `ConstructorParameters<typeof ColouredShape>` reads the
    // subclass's OWN constructor parameter list `(id: string, count: number)`
    // — NOT the base `BaseShape(id: string)`. Both parameters surface in the
    // projected tuple in declaration order.
    //
    // Published surface: `[id: string, count: number]`
    //
    // The mixed primitives (string then number) are deliberate so the
    // assertion can DISCRIMINATE between "found the subclass ctor" and three
    // shortcut implementations:
    //   * "use base ctor only"        — fails len==2 (base has len==1)
    //   * "use base ctor + pad string" — fails on elements[1] type=Number
    //   * "swap argument order"        — fails on elements[0] type=String
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "ColouredShapeCtorParams",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = &expr else {
        panic!("expected tuple of constructor parameters, got {expr:?}");
    };
    assert_eq!(
        elements.len(),
        2,
        "expected the subclass's two ctor params, got {elements:?}"
    );
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_primitive(&elements[1].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not infer return types from subclass method bodies that reference inherited protected members; keep as the future protected-inherited inference contract"]
fn class_features_protected_inherited_member_drives_subclass_method_inference() {
    // TS7 contract: inside `ProtectedConsumer.bumped()` the expression
    // `this.value + "!"` reads the inherited `protected value: string` from
    // base `ProtectedHolder`. The result is string concatenation, so the
    // inferred return type is `string`.
    //
    // `ReturnType<ProtectedConsumer["bumped"]>` = `string`.
    //
    // String concatenation (not numeric `+ 1`) is deliberate: the ONLY way
    // this assertion is satisfied is to genuinely resolve `value` against the
    // base class declaration. A naive "binary `+` → number" arithmetic
    // fallback would yield `number` and fail the assertion.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "ProtectedConsumerBumpedReturn",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not substitute the subclass's own type parameter into the base class's generic type parameter when projecting `Wrapper<string>`; keep as the future generic-subclass-with-own-type-parameter inheritance contract"]
fn class_features_generic_subclass_with_own_type_param_substitutes_through_base() {
    // TS7 contract: `class Wrapper<U> extends Box<U>` declares its own type
    // parameter `U` and forwards it through to the base `Box<T>`. When
    // referenced as `Wrapper<string>`, `U` binds to `string`, which propagates
    // into the inherited `value: T = U = string` field. The subclass adds its
    // own `tag(): "wrapped"` literal-returning method.
    //
    // Published surface: { value: string; tag(): "wrapped" }
    //
    // This is the two-step substitution case: the type argument enters at the
    // subclass binding (`Wrapper<U>`), is passed through to the parent
    // (`extends Box<U>`), and finally substitutes the base's own type parameter
    // `T`. A resolver that only handles the one-step `extends Base<Concrete>`
    // case (see `StringBox extends Box<string>` above) would still miss this
    // and emit `Wrapper<T>` with an unsubstituted `value: U`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "WrapperOfString",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(names, vec!["tag", "value"]);
    assert_primitive(&props["value"].ty, PrimitiveName::String);
    let tag = function_type(&props["tag"].ty);
    assert_string_literal(
        tag.return_type.as_ref().expect("tag() return type"),
        "wrapped",
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate ReturnType against an instantiation expression `typeof Class.method<Concrete>`; keep as the future static-generic-method instantiation contract"]
fn class_features_static_generic_method_instantiation_projects_return_with_substitution() {
    // TS7 contract (instantiation expressions): given
    // `static make<T>(value: T): { wrapped: T }`, the expression
    // `typeof GenericStatic.make<string>` instantiates the generic function
    // with `T = string`, yielding `(value: string) => { wrapped: string }`.
    // `ReturnType<...>` then projects the object return as
    //   `{ wrapped: string }`
    //
    // The deliberately-different field name (`wrapped`, not `value`) and the
    // concrete primitive (`string`, not the original `T`) make the assertion
    // discriminate between "substituted T into the return" and a fallback
    // that returns the unsubstituted shape `{ wrapped: T }`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "StaticMethodInstantiated",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["wrapped"]);
    assert_primitive(&props["wrapped"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_private_field_is_absent_from_published_instance_surface() {
    // TS7 contract: `#secret` is a private brand on `PrivateHolder`. It is
    // NOT a type-level member key — `keyof InstanceType<typeof PrivateHolder>`
    // resolves to `"visible"` alone. The brand exists only to gate
    // assignability between `PrivateHolder` instances at runtime / call sites;
    // it is invisible to the type-level instance projection.
    //
    // Published surface: { visible: string }
    //
    // Negative assertions: neither `"#secret"` nor `"secret"` appear as
    // property names on the projected object. A resolver that accidentally
    // surfaced the `#`-prefixed declaration (or stripped the `#` and surfaced
    // it as `secret`) would fail one of those two negative checks.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "PrivateHolderInstance",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["visible"]);
    assert_primitive(&props["visible"].ty, PrimitiveName::String);
    assert!(
        !props.contains_key("#secret"),
        "published instance must not expose the `#secret` private brand: {props:?}"
    );
    assert!(
        !props.contains_key("secret"),
        "published instance must not expose `secret` (no `#`-stripping): {props:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_keyof_mixed_visibility_reifies_only_public_keys() {
    // TS7 contract: `keyof MixedVis` excludes protected/private members from the
    // keyspace. `Record<keyof MixedVis, 1>` reifies that keyspace into an object
    // surface whose keys are exactly the keyof keys — only the public key `a`.
    //
    // Discrimination: FAILS on a tree where the keyof keyspace enumeration is
    // not visibility-gated — `b` / `c` would enter the keyspace and become keys
    // of the reified Record object.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisRecord",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["a"],
        "Record<keyof MixedVis, 1> keys = the public keyof keyspace `a` only: {props:?}"
    );
    assert!(
        !props.contains_key("b"),
        "protected `b` must NOT be a keyof key: {props:?}"
    );
    assert!(
        !props.contains_key("c"),
        "private `c` must NOT be a keyof key: {props:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_partial_over_mixed_visibility_excludes_non_public_members() {
    // TS7 contract: `Partial<MixedVis>` maps over `keyof MixedVis` (public-only),
    // so the produced surface carries ONLY the public member `a` (optional). The
    // protected/private members are not part of the keyspace and never appear.
    //
    // Discrimination: FAILS on a tree where the mapped-type keyspace is not
    // visibility-gated — `b` / `c` would be produced onto the mapped surface
    // (carried with their non-public visibility).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisPartial",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    let mut names = prop_names(&props);
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["a"],
        "Partial<MixedVis> must produce only the public member `a`: {props:?}"
    );
    assert!(
        !props.contains_key("b"),
        "protected `b` must be absent from Partial<MixedVis>: {props:?}"
    );
    assert!(
        !props.contains_key("c"),
        "private `c` must be absent from Partial<MixedVis>: {props:?}"
    );
    // The surviving member is public.
    assert_eq!(
        props["a"].visibility,
        verter_type_expr::MemberVisibility::Public,
        "the mapped member `a` is public"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_pick_over_mixed_visibility_materialises_only_public_key() {
    // TS7 contract: `Pick<MixedVis, "a">` materialises only the public member
    // `a`. (A non-public key is not a valid keyof member, so it could not be
    // picked.)
    //
    // Discrimination: FAILS if Pick/member-route reconstruction surfaces a
    // non-public member, or if the keyspace gate is absent and `b`/`c` leak in.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisPick",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["a"],
        "Pick<MixedVis, \"a\"> must materialise only `a`: {props:?}"
    );
    assert_eq!(
        props["a"].visibility,
        verter_type_expr::MemberVisibility::Public,
        "the picked member `a` is public"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_union_common_member_folds_to_most_restrictive_visibility() {
    // TS member-access surface of an ordinary (non-macro) union `UnionA | UnionB`
    // is the COMMON members only, and each common member's accessibility folds to
    // the MOST RESTRICTIVE across the arms. `shared` is public in UnionA but
    // private in UnionB, so the merged `shared` is private. Arm-only members
    // (`onlyA` / `onlyB`) are not common and are absent.
    //
    // Discrimination: FAILS on a tree where `merge_union_surfaces` hardcodes
    // `Public` for the union common-member (the pre-fix walk.rs behaviour) — the
    // merged `shared` would be Public instead of Private.
    let host = make_host_with_footprint();
    upsert(&host);

    let expr = shallow_surface_expr(&host, "/fixtures/class_features.ts", "UnionAB");

    let props = object_props(&expr);
    assert!(
        props.contains_key("shared"),
        "the common member `shared` survives the union surface: {props:?}"
    );
    assert!(
        !props.contains_key("onlyA"),
        "arm-only `onlyA` is not a common member: {props:?}"
    );
    assert!(
        !props.contains_key("onlyB"),
        "arm-only `onlyB` is not a common member: {props:?}"
    );
    assert_eq!(
        props["shared"].visibility,
        verter_type_expr::MemberVisibility::Private,
        "union common-member `shared` folds to the most-restrictive (Private) \
         visibility — public in UnionA, private in UnionB: {:?}",
        props["shared"]
    );
}
