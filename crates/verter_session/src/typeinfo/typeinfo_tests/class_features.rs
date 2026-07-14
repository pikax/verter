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

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

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

// LIFTED: `typeof StepCounter.initial` walks the static-heritage chain to the
// base `BaseCounter.initial: string` declared annotation (the static
// composer folds base statics under own-shadows-base precedence). The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn class_features_static_inheritance_resolves_inherited_field_type() {}

// LIFTED: `ReturnType<typeof StepCounter.describe>` resolves the INHERITED static
// method through the static-heritage composer and projects its declared
// `string` return. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn class_features_static_inheritance_resolves_inherited_method_return() {}

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
#[ignore = "reducer composes the two-hop dual-space substitution correctly (Verter expands `Wrapper<string>` to `{ tag(): \"wrapped\" } & { value: string }`) but the row is NOT oracle-liftable — tsgo's hover displays a generic class instance type NOMINALLY (`Wrapper<string>`, a bare ref), so the snapshot value cannot discriminate the structural substitution the row contracts (measured ValueMismatch: verter structural vs oracle nominal ref). Lift pending an oracle probe/grammar extension that elicits structural display for class instance types"]
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

// LIFTED: `ReturnType<typeof GenericStatic.make<string>>` lowers the
// instantiation-expression type args on the typeof path and instantiates
// the static generic method to `{ wrapped: string }`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn class_features_static_generic_method_instantiation_projects_return_with_substitution() {}

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
    // POSITIVE CONTROL (non-discriminating by design): `Pick<MixedVis, "a">`
    // materialises the public member `a`. This does NOT discriminate the Pick
    // public-keyspace gate — `a` is public and `b` / `c` are not in the pick key
    // set, so it passes whether or not Pick public-filters its source members.
    // It is retained to prove the happy-path public Pick still materialises. The
    // DISCRIMINATING coverage for the Pick gate (a `Pick` whose key names a
    // NON-public member must materialise an EMPTY surface) lives in
    // `class_features_pick_over_mixed_visibility_protected_key_is_empty` /
    // `..._private_key_is_empty` below.
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
fn class_features_pick_over_mixed_visibility_protected_key_is_empty() {
    // DISCRIMINATING (fix #1, Pick public-keyspace gate): `Pick<MixedVis, "b">`
    // where `b` is PROTECTED. `b` ∉ `keyof MixedVis` (public-only), so the
    // picked surface is EMPTY — no member is materialised.
    //
    // Discrimination: FAILS on the pre-fix tree where `build_builtin_utility`'s
    // Pick arm filters `object_filter_source_surface`'s FULL surface (all
    // members, including non-public) by NAME only — `b` matches and is re-minted
    // onto the surface. PASSES once Pick public-filters its source members before
    // the name predicate.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisPickProtected",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props_or_empty(&expr);
    assert!(
        !props.contains_key("b"),
        "Pick<MixedVis, \"b\"> over a PROTECTED key must NOT materialise `b`: {expr:?}"
    );
    assert!(
        props.is_empty(),
        "Pick<MixedVis, \"b\"> materialises an EMPTY surface (b ∉ keyof MixedVis): {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_pick_over_mixed_visibility_private_key_is_empty() {
    // DISCRIMINATING (fix #1, Pick public-keyspace gate): `Pick<MixedVis, "c">`
    // where `c` is PRIVATE. `c` ∉ `keyof MixedVis`, so the picked surface is
    // EMPTY.
    //
    // Discrimination: FAILS on the pre-fix tree (private `c` leaks through the
    // name-only Pick filter); PASSES once Pick public-filters its source members.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisPickPrivate",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props_or_empty(&expr);
    assert!(
        !props.contains_key("c"),
        "Pick<MixedVis, \"c\"> over a PRIVATE key must NOT materialise `c`: {expr:?}"
    );
    assert!(
        props.is_empty(),
        "Pick<MixedVis, \"c\"> materialises an EMPTY surface (c ∉ keyof MixedVis): {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_omit_over_mixed_visibility_does_not_leave_non_public() {
    // DISCRIMINATING (fix #1, Omit public-keyspace gate): `Omit<MixedVis, "a">`
    // = `Pick<MixedVis, Exclude<keyof MixedVis, "a">>`. `keyof MixedVis` is `"a"`
    // only (public), so excluding `"a"` leaves the EMPTY public keyspace — the
    // non-public `b` / `c` must NOT survive into the omitted surface.
    //
    // Discrimination: FAILS on the pre-fix tree where Omit keeps every source
    // member whose name is not omitted — `b` / `c` survive the name-only filter
    // and are re-minted onto the surface. PASSES once Omit public-filters its
    // source members before the name predicate.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisOmitPublic",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props_or_empty(&expr);
    assert!(
        !props.contains_key("b"),
        "Omit<MixedVis, \"a\"> must NOT leave protected `b` on the surface: {expr:?}"
    );
    assert!(
        !props.contains_key("c"),
        "Omit<MixedVis, \"a\"> must NOT leave private `c` on the surface: {expr:?}"
    );
    assert!(
        props.is_empty(),
        "Omit<MixedVis, \"a\"> over a public-only keyspace yields an EMPTY surface: {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn class_features_direct_indexed_access_private_key_does_not_leak_value() {
    // NON-LEAK CHARACTERIZATION (fix #2 surface; the discriminating unit test is
    // `project_semantic_dispatch::tests::
    //  project_member_rejects_non_public_members_from_external_surface`):
    // `MixedVis["c"]` indexes a PRIVATE class member. External index access of a
    // non-public member is not allowed in TS. The current resolver leaves a
    // direct indexed access over a BARE class reference (`MixedVis` resolves to a
    // `DeclRef` carrier, not yet an Object) as a DEFERRED `IndexedAccess` carrier
    // — the same pre-existing behavior the `Dog["sound"]` /
    // `ProtectedConsumer["bumped"]` tests carry `#[ignore]` for (class-member
    // indexed access does not reduce on a bare class ref here). The B4.5-relevant
    // invariant this test pins is the NON-LEAK property: the private member's
    // value type (`boolean`) must NEVER be the result.
    //
    // The DISCRIMINATING coverage for fix #2 (the `advance_step` object member
    // lookup rejecting a non-public member once the base IS an Object) is the
    // dispatch-level unit test named above, which builds the Object surface
    // directly and asserts the Opaque miss.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, _record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisIndexedPrivate",
        &[],
        ProjectionMode::Expanded,
    );

    // The result must NOT be the private member's value type, and must carry no
    // `c` member anywhere — it stays an opaque/deferred carrier.
    assert!(
        !matches!(&expr, TypeExpr::Primitive(PrimitiveName::Boolean)),
        "MixedVis[\"c\"] must NOT resolve to the private member's value type `boolean`: {expr:?}"
    );
    assert!(
        !object_props_or_empty(&expr).contains_key("c"),
        "MixedVis[\"c\"] must not surface the private member `c`: {expr:?}"
    );
}

#[test]
fn class_features_imported_class_fact_fast_path_excludes_non_public_keyspace() {
    // END-TO-END REGRESSION (fix #4 fact fast path + fix #2 backstop): a class
    // IMPORTED from another file resolves to a cross-file `DeclRef` carrier that
    // the mapped-narrowing walker does NOT eagerly enumerate. Indexing a mapped
    // type over that import (`Partial<Imported>["secret"]`) routes key admission
    // through the NON-EMITTING `MemberPresence` fact fast path (walk.rs Tier-2 →
    // `keyspace_admits_literal_non_emitting` → `base_member_admission_non_emitting`
    // → `member_presence_fact_admission`). The private member `secret` must NOT
    // narrow to a value.
    //
    // This test pins the cross-file construct staying CLEAN end-to-end. It does
    // NOT discriminate fix #4 in ISOLATION: even if the fact fast path wrongly
    // admitted `secret` from presence alone, the narrowing then re-dispatches
    // `Imported["secret"]`, which hits fix #2's `advance_step` public gate and
    // still misses — fix #2 is the architectural backstop. The DISCRIMINATING
    // unit test that pins fix #4's behavior in isolation (a present member is
    // INCONCLUSIVE, not `Some(true)`) is `project_semantic_dispatch::tests::
    // base_member_admission_fact_fast_path_is_inconclusive_for_present_members`.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/imported_class.ts",
        r#"export class Imported {
  public open: string = "";
  protected guarded: number = 0;
  private secret: boolean = false;
}
"#,
    );
    upsert_ts(
        &host,
        "/fixtures/imported_class_consumer.ts",
        r#"import { Imported } from "./imported_class";
// @ts-expect-error 'secret' is private; not a public key of Partial<Imported>
export type ImportedIndexedSecret = Partial<Imported>["secret"];
// @ts-expect-error 'guarded' is protected; not a public key of Partial<Imported>
export type ImportedIndexedGuarded = Partial<Imported>["guarded"];
export type ImportedIndexedOpen = Partial<Imported>["open"];
"#,
    );

    // `Partial<Imported>["open"]` (public) narrows to the public member's value
    // — positive control that the cross-file mapped+indexed path works.
    let (open_expr, _r0) = resolve_expr(
        &host,
        "/fixtures/imported_class_consumer.ts",
        "ImportedIndexedOpen",
        &[],
        ProjectionMode::Expanded,
    );
    assert!(
        !open_expr.is_unknown(),
        "Partial<Imported>[\"open\"] (public) must narrow, not miss: {open_expr:?}"
    );

    // `Partial<Imported>["secret"]` (private) / `["guarded"]` (protected): the
    // non-public key is not in `keyof Imported`, so the mapped narrowing must NOT
    // produce the member's value type — it is a miss.
    for (alias, leaked) in [
        ("ImportedIndexedSecret", PrimitiveName::Boolean),
        ("ImportedIndexedGuarded", PrimitiveName::Number),
    ] {
        let (expr, _r) = resolve_expr(
            &host,
            "/fixtures/imported_class_consumer.ts",
            alias,
            &[],
            ProjectionMode::Expanded,
        );
        assert!(
            !matches!(&expr, TypeExpr::Primitive(p) if *p == leaked),
            "{alias} (non-public key via the fact fast path) must NOT narrow to the \
             member's value type {leaked:?}: {expr:?}"
        );
        assert!(
            expr.is_unknown(),
            "{alias} (non-public key out of a public-only mapped surface) must be a \
             semanticMiss: {expr:?}"
        );
    }
}

#[test]
fn class_features_nested_mapped_indexed_access_private_key_is_miss() {
    // DISCRIMINATING (fix #3, mapped/indexed admission): `Partial<MixedVis>["c"]`
    // indexes a PRIVATE key out of a mapped surface over a class. `Partial<…>`
    // maps over `keyof MixedVis` (public-only), so it carries no `c` member and
    // the indexed access is a miss.
    //
    // Discrimination: FAILS on a tree where the mapped/indexed admission
    // (`walk.rs` Tier-1 Object membership or `base_member_admission_non_emitting`)
    // admits `c` by NAME only and forges a value type. PASSES once the object
    // admission requires `is_public()`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, _record) = resolve_expr(
        &host,
        "/fixtures/class_features.ts",
        "MixedVisPartialIndexedPrivate",
        &[],
        ProjectionMode::Expanded,
    );

    assert!(
        !matches!(&expr, TypeExpr::Primitive(PrimitiveName::Boolean)),
        "Partial<MixedVis>[\"c\"] must NOT resolve to the private member's value type: {expr:?}"
    );
    assert!(
        expr.is_unknown(),
        "Partial<MixedVis>[\"c\"] (private key out of a public-only mapped surface) \
         must be a semanticMiss: {expr:?}"
    );
}

#[test]
fn class_features_heritage_base_identities_and_args_resolve_same_file_and_cross_file() {
    // Static-heritage base resolution over BOTH base kinds in one consumer:
    //
    //   * `CrossDerived` is ctor-LESS and extends the IMPORTED
    //     `WideBase<string>` — the Static composer resolves the heritage base
    //     CROSS-FILE (through the class decl's `name_resolution`), lowers the
    //     heritage type-argument on demand, and the inherited constructor's
    //     `value: T` parameter surfaces with `T = string` substituted.
    //   * `LocalDerived` extends the SAME-FILE userland `LocalBase` — the
    //     same-file base identity resolves through the same rail and its
    //     static `local: number` folds onto the derived static surface.
    //
    // Discrimination:
    //   * `CrossTag = string` FAILS if the cross-file base head does not
    //     resolve (the inherited static would be a miss).
    //   * `CrossCtorParams[0] = string` FAILS if the heritage type-argument
    //     is not lowered/substituted (an unbound `T` cannot produce `string`);
    //     `[1] = number` pins the base's own second parameter.
    //   * `LocalStatic = number` FAILS if the same-file base head resolution
    //     regresses.
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/heritage_wide_base.ts",
        r#"export class WideBase<T> {
  static tag: string = "";
  constructor(value: T, count: number) {}
}
"#,
    );
    upsert_ts(
        &host,
        "/fixtures/heritage_consumer.ts",
        r#"import { WideBase } from "./heritage_wide_base";
class LocalBase {
  static local: number = 1;
}
export class CrossDerived extends WideBase<string> {}
export class LocalDerived extends LocalBase {}
export type CrossCtorParams = ConstructorParameters<typeof CrossDerived>;
export type CrossTag = typeof CrossDerived.tag;
export type LocalStatic = typeof LocalDerived.local;
"#,
    );

    // Cross-file base statics fold into the derived static surface.
    let (tag_expr, _record) = resolve_expr(
        &host,
        "/fixtures/heritage_consumer.ts",
        "CrossTag",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&tag_expr, PrimitiveName::String);

    // The ctor-less subclass inherits the base constructor's parameters with
    // the heritage type-argument substituted: `[value: string, count: number]`.
    let (params_expr, _record) = resolve_expr(
        &host,
        "/fixtures/heritage_consumer.ts",
        "CrossCtorParams",
        &[],
        ProjectionMode::Expanded,
    );
    let TypeExpr::Tuple { elements, .. } = &params_expr else {
        panic!("expected tuple of inherited constructor parameters, got {params_expr:?}");
    };
    assert_eq!(
        elements.len(),
        2,
        "expected the base ctor's two params, got {elements:?}"
    );
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_primitive(&elements[1].ty, PrimitiveName::Number);

    // Same-file userland base identity resolves through the same rail.
    let (local_expr, _record) = resolve_expr(
        &host,
        "/fixtures/heritage_consumer.ts",
        "LocalStatic",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&local_expr, PrimitiveName::Number);
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
