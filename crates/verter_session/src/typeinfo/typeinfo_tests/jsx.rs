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
//! Verter resolves these ambient global-`JSX`-namespace member types. The
//! `declare global { namespace JSX { ... } }` declarations resolve through the
//! shared resolver's global-augmentation fallback, and the qualified
//! `JSX.IntrinsicElements[Tag]`, `keyof JSX.IntrinsicElements`, and `JSX.Element`
//! lookups reduce over the merged namespace surface. The two parametric
//! `IntrinsicPropsFor<"div">` / `IntrinsicPropsFor<"span">` rows are oracle-lifted
//! (re-homed to `U2.INDEXED_ACCESS`); the rest engine-resolve under
//! `--include-ignored`. The lone exception is the component-factory row
//! (`Parameters<typeof createElement<{ label: string }>>[1]`), a disclosed
//! `U6.CALL_RESOLVE` `ResolveCall` / `InferTypeArgs` gap that returns a clean
//! `semanticMiss` (no hang).

use super::oracle;
use super::support::*;
use crate::VerterHost;
use verter_session_oracle_macro::oracle_row;

const JSX_FIXTURE: &str = include_str!("fixtures/jsx.ts");
const PATH_JSX: &str = "/fixtures/jsx.ts";

const JSX_NS_SIBLING_FIXTURE: &str = include_str!("fixtures/jsx_namespace_sibling.ts");
const PATH_JSX_NS_SIBLING: &str = "/fixtures/jsx_namespace_sibling.ts";

fn upsert_jsx(host: &VerterHost) {
    upsert_ts(host, PATH_JSX, JSX_FIXTURE);
}

// ---------------------------------------------------------------------------
// 1) `JSX.IntrinsicElements["tag"]` direct index resolution
// ---------------------------------------------------------------------------

#[test]
#[ignore = "JSX foundations reducer complete: Verter resolves `JSX.IntrinsicElements[\"div\"]` against the `declare global { namespace JSX { interface IntrinsicElements { ... } } }` declaration to the declared shape `{ id?: string; className?: string }` (verified under --include-ignored). NOT oracle-liftable — the qualified-namespace indexed-access source body `JSX.IntrinsicElements[\"div\"]` lowers to a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"indexed-access\"))); lift pending a qualified-namespace indexed-access source-walk carve-out"]
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
#[ignore = "JSX foundations reducer complete: Verter resolves `JSX.IntrinsicElements[\"span\"]` against the `declare global { namespace JSX { interface IntrinsicElements { ... } } }` declaration to the declared shape `{ title?: string }` (verified under --include-ignored). NOT oracle-liftable — the qualified-namespace indexed-access source body `JSX.IntrinsicElements[\"span\"]` lowers to a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"indexed-access\"))); lift pending a qualified-namespace indexed-access source-walk carve-out"]
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
#[ignore = "JSX namespace resolution is complete, but this row additionally needs a later-block mechanism U2 does not own: `Parameters<typeof createElement<{ label: string }>>[1]` requires `typeof createElement<{ label: string }>` — an explicit-type-argument instantiation of a generic FUNCTION VALUE — which dispatches `ResolveCall` / `InferTypeArgs` owned by U6.CALL_RESOLVE (U2.JSX_FOUNDATIONS precedes U6 and cannot consume ResolveCall). Verter therefore returns a clean `semanticMiss` (no hang) for the unreduced `Parameters<...>` object, so this row does NOT yet resolve under --include-ignored; the oracle preflight is correspondingly unclean (PreflightUnclean -> Reject(DeferredConstruct(\"indexed-access\")) over `IndexedAccess { object: Unknown(semanticMiss), index: 1 }`). Lift pending the U6 ResolveCall mechanism"]
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
#[ignore = "JSX foundations reducer complete: Verter resolves `Parameters<LabelFC>[0]` (where `LabelFC = FC<{ label: string }>`, `FC<P> = (props: P & { children?: unknown }) => JSX.Element`) to the intersected props surface `{ children?: unknown; label: string }` (verified under --include-ignored). NOT oracle-liftable — the numeric indexed-access source body `Parameters<LabelFC>[0]` lowers to a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"indexed-access\"))); lift pending a numeric-index source-walk carve-out"]
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

// LIFTED: `DivPropsViaIndex = IntrinsicPropsFor<"div">` (alias for
// `JSX.IntrinsicElements[Tag]`) instantiates `Tag = "div"` and reduces the
// indexed access over the global-augmented `JSX.IntrinsicElements` to the
// declared `div` shape `{ id?: string; className?: string }`. The lifted body
// is the registry-keyed `oracle::run_row` shared-driver comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot. The source body
// is a bare `Ref` carrying a string-literal type argument (the source walk does
// NOT descend the alias body), so the oracle gate admits it. Trace dispatches
// `ResolveDecl` + `Instantiate` + `IndexedAccess`, re-homing the row to
// `U2.INDEXED_ACCESS` (terminal `IndexedAccessUnionDistribution`).
#[oracle_row]
#[test]
fn jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape() {}

#[test]
#[ignore = "JSX foundations reducer complete: Verter resolves `keyof JSX.IntrinsicElements` to the cross-block-merged string-literal union `\"customCard\" | \"div\" | \"span\"` (verified under --include-ignored). NOT oracle-liftable — the qualified-namespace keyof source body `keyof JSX.IntrinsicElements` lowers to a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"keyof\"))); lift pending a qualified-namespace keyof source-walk carve-out"]
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

// LIFTED: `SpanPropsViaIndex = IntrinsicPropsFor<"span">` reduces through
// `JSX.IntrinsicElements["span"]` to the declared `span` shape
// `{ title?: string }` — sibling intrinsic-element keys (`div`, `customCard`)
// must not leak in. Same lift mechanics as the `div` parametric-lookup row:
// the source body is a bare `Ref` with a string-literal type argument (admitted
// by the oracle gate), the trace dispatches `ResolveDecl` + `Instantiate` +
// `IndexedAccess`, and the row re-homes to `U2.INDEXED_ACCESS`.
#[oracle_row]
#[test]
fn jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape() {}

// ---------------------------------------------------------------------------
// 6) Augmented `JSX.IntrinsicElements` — multiple `declare global` blocks
//    merge.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "JSX foundations reducer complete: Verter merges the two `declare global { namespace JSX { interface IntrinsicElements { ... } } }` blocks via the shared MergedDecl peer-merge and resolves `JSX.IntrinsicElements[\"customCard\"]` to the augmented shape `{ variant?: \"primary\" | \"secondary\" }` (verified under --include-ignored). NOT oracle-liftable — the qualified-namespace indexed-access source body `JSX.IntrinsicElements[\"customCard\"]` lowers to a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"indexed-access\"))); lift pending a qualified-namespace indexed-access source-walk carve-out"]
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
#[ignore = "JSX foundations reducer complete: Verter resolves `JSX.Element` against the `declare global { namespace JSX { interface Element { ... } } }` declaration to the declared interface shape `{ __element_brand__: true }` (verified under --include-ignored). NOT oracle-liftable — tsgo's Expanded hover prints the qualified alias name `JSX.Element` rather than the structural surface, and the oracle gate rejects a qualified-name ref (oracle admission Reject(EnumMemberOrQualified)); lift pending a qualified-name hover carve-out"]
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

// ---------------------------------------------------------------------------
// Global-namespace sibling resolution
// ---------------------------------------------------------------------------
//
// Targeted regression test (a plain `#[test]`, NOT a tsgo-parity oracle row
// or an ignored manifest row): an unqualified reference to a namespace-LOCAL
// sibling inside a `declare global { namespace JSX { ... } }` body must
// resolve through the global-augmentation sibling scope `(Global,
// "JSX.Common")`, exactly as the file-scope `declare namespace JSX` path
// already binds its siblings. Before the augmentation-scope sibling binding,
// the body reference `Common` (retained under `(Global, "JSX.Common")`, never
// in file-scope `type_symbols`) failed to resolve, so the indexed access did
// not reach the declared `{ id?: string }` shape.

#[test]
fn jsx_intrinsic_member_resolves_through_global_namespace_sibling() {
    // Contract: `DivIntrinsic = JSX.IntrinsicElements["div"]` where the
    //   member is `div: Common` and `type Common = { id?: string }` is a
    //   sibling in the same global-augmented `namespace JSX`. The indexed
    //   access dereferences `Common` through the `(Global, "JSX.Common")`
    //   augmentation sibling scope, yielding `{ id?: string }`.
    let host = make_host_with_footprint();
    upsert_ts(&host, PATH_JSX_NS_SIBLING, JSX_NS_SIBLING_FIXTURE);

    let (expr, record) = resolve_expr(
        &host,
        PATH_JSX_NS_SIBLING,
        "DivIntrinsic",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert!(props["id"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
