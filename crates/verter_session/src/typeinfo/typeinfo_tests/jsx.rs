//! @ai-generated - JSX-namespace typeinfo contracts.
//!
//! Encodes the JSX-shaped surfaces used by component-factory inference,
//! React-FC-equivalent expansion, and parametric intrinsic-element
//! lookup. The fixture is a `.ts` file (not `.tsx`) — every JSX shape
//! is expressed through `JSX.IntrinsicElements` / `JSX.Element` types,
//! never literal JSX tags. This isolates the resolver contracts from
//! the JSX emit mode.
//!
//! Also exercises augmented `JSX.IntrinsicElements` via a second
//! `declare global { namespace JSX { interface IntrinsicElements { ... } } }`
//! block, and direct projection of `JSX.Element` to its declared
//! interface shape.
//!
//! All contracts are TDD-red future targets — Verter does not currently
//! resolve through the global `JSX` namespace nor reduce parametric
//! `JSX.IntrinsicElements[Tag]` lookups.

use super::support::*;
use crate::VerterHost;

const JSX_FIXTURE: &str = include_str!("fixtures/jsx.ts");
const PATH_JSX: &str = "/fixtures/jsx.ts";

fn upsert_jsx(host: &VerterHost) {
    upsert_ts(host, PATH_JSX, JSX_FIXTURE);
}

// ---------------------------------------------------------------------------
// 1) `JSX.IntrinsicElements["tag"]` direct index resolution
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not resolve `JSX.IntrinsicElements['div']` against a `declare global { namespace JSX { interface IntrinsicElements { ... } } }` declaration; keep as the future JSX intrinsic-element lookup contract"]
fn jsx_intrinsic_div_resolves_to_declared_shape() {
    // TS7 contract: `DivIntrinsic = JSX.IntrinsicElements["div"]` =
    //   `{ id?: string; className?: string }`. Both members optional.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "DivIntrinsic",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["className", "id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["className"].ty, PrimitiveName::String);
    assert!(props["id"].optional);
    assert!(props["className"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not resolve `JSX.IntrinsicElements['span']` against a `declare global { namespace JSX { interface IntrinsicElements { ... } } }` declaration; keep as the future JSX intrinsic-element lookup contract"]
fn jsx_intrinsic_span_resolves_to_declared_shape() {
    // TS7 contract: `SpanIntrinsic = JSX.IntrinsicElements["span"]` =
    //   `{ title?: string }`.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "SpanIntrinsic",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["title"]);
    assert_primitive(&props["title"].ty, PrimitiveName::String);
    assert!(props["title"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 2) Component-factory generic-prop inference
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not infer the parametric `P` of `createElement<P>(component, props)` from a fully-typed component argument when projected through `Parameters<typeof createElement<...>>[1]`; keep as the future JSX factory inferred-props contract"]
fn jsx_factory_inferred_props_for_component_resolves() {
    // TS7 contract: For
    //   `createElement<P>(component: (props: P) => JSX.Element, props: P): JSX.Element`
    //   instantiated as `typeof createElement<{ label: string }>`,
    //   `Parameters<...>[1]` reduces to the second-parameter type =
    //   `{ label: string }`.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "InferredPropsForMyComponent",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["label"]);
    assert_primitive(&props["label"].ty, PrimitiveName::String);
    assert!(!props["label"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 3) React.FC<P> equivalent — props expansion with optional children
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not project `Parameters<FC<P>>[0]` where `FC<P> = (props: P & { children?: unknown }) => JSX.Element` into the intersected props surface with an optional `children`; keep as the future React.FC-equivalent expansion contract"]
fn jsx_fc_props_includes_children_optional() {
    // TS7 contract: `LabelFCProps = Parameters<LabelFC>[0]` where
    //   `LabelFC = FC<{ label: string }>` = `(props: { label: string } &
    //   { children?: unknown }) => JSX.Element`. The reduced first-
    //   parameter type is `{ label: string; children?: unknown }`.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "LabelFCProps",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["children", "label"]);
    assert_primitive(&props["label"].ty, PrimitiveName::String);
    assert!(!props["label"].optional);
    assert!(props["children"].optional);
    assert_primitive(&props["children"].ty, PrimitiveName::Unknown);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 4) Parametric `JSX.IntrinsicElements[Tag]` lookup
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not specialize `IntrinsicPropsFor<Tag>` (alias for `JSX.IntrinsicElements[Tag]`) when the type argument selects a concrete intrinsic key; keep as the future JSX parametric intrinsic-lookup contract"]
fn jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape() {
    // TS7 contract: `DivPropsViaIndex = IntrinsicPropsFor<"div">` reduces
    //   through `JSX.IntrinsicElements["div"]` to the declared `div` shape
    //   `{ id?: string; className?: string }`.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "DivPropsViaIndex",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["className", "id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["className"].ty, PrimitiveName::String);
    assert!(props["id"].optional);
    assert!(props["className"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not surface `keyof JSX.IntrinsicElements` as a merged string-literal union over the declared + augmented intrinsic-element keys; keep as the future JSX intrinsic-keys-after-augmentation contract"]
fn jsx_intrinsic_keys_resolves_to_string_literal_union() {
    // TS7 contract: `IntrinsicKeys = keyof JSX.IntrinsicElements` =
    //   `"div" | "span" | "customCard"` — a three-arm string-literal union
    //   including the augmented key contributed by the second
    //   `declare global { namespace JSX { interface IntrinsicElements { ... } } }`
    //   block in the fixture. Declaration merging across both blocks must be
    //   reflected in `keyof`.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "IntrinsicKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["customCard", "div", "span"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 5) `IntrinsicPropsFor<"span">` parametric specialisation
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not specialize `IntrinsicPropsFor<Tag>` (alias for `JSX.IntrinsicElements[Tag]`) when the type argument selects the `\"span\"` intrinsic key; keep as the future JSX parametric intrinsic-lookup contract"]
fn jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape() {
    // TS7 contract: `SpanPropsViaIndex = IntrinsicPropsFor<"span">` reduces
    //   through `JSX.IntrinsicElements["span"]` to the declared `span`
    //   shape `{ title?: string }`. Sibling intrinsic-element keys (`div`,
    //   `customCard`) must not leak in.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "SpanPropsViaIndex",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["title"]);
    assert_primitive(&props["title"].ty, PrimitiveName::String);
    assert!(props["title"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 6) Augmented `JSX.IntrinsicElements` — multiple `declare global` blocks
//    merge.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not merge multiple `declare global { namespace JSX { interface IntrinsicElements { ... } } }` blocks into the same JSX intrinsic-element surface; keep as the future JSX intrinsic-augmentation contract"]
fn jsx_intrinsic_augmented_custom_card_resolves_to_declared_shape() {
    // TS7 contract: A second `declare global { namespace JSX { interface
    //   IntrinsicElements { customCard: ... } } }` block merges with the
    //   original. `JSX.IntrinsicElements["customCard"]` therefore resolves
    //   to `{ variant?: "primary" | "secondary" }` — the new entry — while
    //   the original `div` / `span` entries remain visible.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "CustomCardIntrinsic",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["variant"]);
    assert_literal_union(&props["variant"].ty, &["primary", "secondary"]);
    assert!(props["variant"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 7) `JSX.Element` directly projected
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not resolve `JSX.Element` through a `declare global { namespace JSX { interface Element { ... } } }` declaration into the declared interface shape; keep as the future JSX element-shape contract"]
fn jsx_element_resolves_to_declared_interface_shape() {
    // TS7 contract: `ElementShape = JSX.Element` resolves to the declared
    //   `interface Element { __element_brand__: true }` — a single
    //   readonly-by-default property whose type is the literal `true`.
    let host = make_host_with_footprint();
    upsert_jsx(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX,
        "ElementShape",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["__element_brand__"]);
    assert_boolean_literal(&props["__element_brand__"].ty, true);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
