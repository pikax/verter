//! @ai-generated - Module-feature typeinfo contracts.
//!
//! Covers TS7 surface areas the existing typeinfo tests do not exercise:
//!   * Single-level `namespace Geometry { ... }` qualified member lookup.
//!   * Nested `namespace A.B.C { ... }` deep qualified-name resolution.
//!   * `declare global { interface ... }` with multiple `declare global`
//!     blocks merging into a single global interface surface.
//!   * `typeof import("./other")` projection to `default`, named-export
//!     shapes (`["LeafShape"]`), and named-export values (`["leafName"]`).
//!   * `declare module "./..." { interface ... }` interface-merging
//!     augmentation across two files.
//!   * `typeof import("./cjs")` against a CommonJS-style `export = `
//!     ambient module — the value type of the export-= binding.
//!   * Mixed `import { type SomeType, valueExport } from "./leaf"` —
//!     type-only and value-only specifiers on the same import statement.
//!   * Namespace + interface name-merging (`interface X { ... }` +
//!     `namespace X { ... }`) — the type and value sides coexist under
//!     the same identifier.
//!   * `declare module "external-spec" { interface Config { ... } }` —
//!     string-literal ambient module augmentation across two files (the
//!     canonical Vue/Vite `vite/client` pattern with a virtual module
//!     name rather than a relative path).
//!
//! Most of these contracts represent TDD-red future targets — Verter
//! currently does not implement namespace member resolution, global
//! augmentation merge, module-augmentation surface merge, or typeof-
//! import projection. Each unsupported test carries a precise `#[ignore]`
//! reason describing the future contract.

use super::oracle;
use super::support::*;
use crate::VerterHost;
use verter_session_oracle_macro::oracle_row;

const MODULE_FEATURES: &str = include_str!("fixtures/module_features.ts");
const MODULE_FEATURES_LEAF: &str = include_str!("fixtures/module_features_leaf.ts");
const MODULE_FEATURES_BASE: &str = include_str!("fixtures/module_features_base.ts");
const MODULE_FEATURES_PATCH: &str = include_str!("fixtures/module_features_patch.ts");
const MODULE_FEATURES_CJS: &str = include_str!("fixtures/module_features_cjs.d.ts");
const MODULE_FEATURES_CONSUMER: &str = include_str!("fixtures/module_features_consumer.ts");
const MODULE_FEATURES_EXTERNAL: &str = include_str!("fixtures/module_features_external.d.ts");
const MODULE_FEATURES_EXTERNAL_PATCH: &str =
    include_str!("fixtures/module_features_external_patch.ts");
const MODULE_FEATURES_EXTERNAL_CONSUMER: &str =
    include_str!("fixtures/module_features_external_consumer.ts");

const PATH_MAIN: &str = "/fixtures/module_features.ts";
const PATH_LEAF: &str = "/fixtures/module_features_leaf.ts";
const PATH_BASE: &str = "/fixtures/module_features_base.ts";
const PATH_PATCH: &str = "/fixtures/module_features_patch.ts";
const PATH_CJS: &str = "/fixtures/module_features_cjs.d.ts";
const PATH_CONSUMER: &str = "/fixtures/module_features_consumer.ts";
const PATH_EXTERNAL: &str = "/fixtures/module_features_external.d.ts";
const PATH_EXTERNAL_PATCH: &str = "/fixtures/module_features_external_patch.ts";
const PATH_EXTERNAL_CONSUMER: &str = "/fixtures/module_features_external_consumer.ts";

fn upsert_main(host: &VerterHost) {
    upsert_ts(host, PATH_MAIN, MODULE_FEATURES);
}

fn upsert_consumer_graph(host: &VerterHost) {
    upsert_ts(host, PATH_LEAF, MODULE_FEATURES_LEAF);
    upsert_ts(host, PATH_BASE, MODULE_FEATURES_BASE);
    upsert_ts(host, PATH_PATCH, MODULE_FEATURES_PATCH);
    upsert_ts(host, PATH_CJS, MODULE_FEATURES_CJS);
    upsert_ts(host, PATH_CONSUMER, MODULE_FEATURES_CONSUMER);
}

fn upsert_external_graph(host: &VerterHost) {
    upsert_ts(host, PATH_EXTERNAL, MODULE_FEATURES_EXTERNAL);
    upsert_ts(host, PATH_EXTERNAL_PATCH, MODULE_FEATURES_EXTERNAL_PATCH);
    upsert_ts(
        host,
        PATH_EXTERNAL_CONSUMER,
        MODULE_FEATURES_EXTERNAL_CONSUMER,
    );
}

// ---------------------------------------------------------------------------
// Single-level namespace
// ---------------------------------------------------------------------------

#[test]
fn module_features_namespace_geometry_point_resolves_to_shape() {
    // TS7 contract: `GeometryPoint = Geometry.Point` =
    //   `{ x: number; y: number }`.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_MAIN,
        "GeometryPoint",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["x", "y"]);
    assert_primitive(&props["x"].ty, PrimitiveName::Number);
    assert_primitive(&props["y"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `Geometry.Vector` (which aliases `Geometry.Point`) collapses the
// namespace-qualified alias chain to the underlying `{ x: number; y: number }`
// shape. The lifted body is the registry-keyed `oracle::run_row` shared-driver
// call comparing Verter's `Expanded` projection against the checked-in tsgo
// snapshot. Trace dispatches only `ResolveDecl` + `Instantiate`, re-homing the
// row to `U2.QUERY_VALUE_DOMAIN`.
#[oracle_row]
#[test]
fn module_features_namespace_geometry_vector_aliases_point() {}

// ---------------------------------------------------------------------------
// Nested namespace
// ---------------------------------------------------------------------------

#[test]
fn module_features_nested_namespace_leaf_resolves_to_shape() {
    // TS7 contract: `LeafValue = Layer.Inner.Leaf.Value` =
    //   `{ tag: "leaf"; depth: number }`. The resolver must walk every
    //   nested namespace boundary to reach the terminal type.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(&host, PATH_MAIN, "LeafValue", &[], ProjectionMode::Expanded);

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["depth", "tag"]);
    assert_string_literal(&props["tag"].ty, "leaf");
    assert_primitive(&props["depth"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// `declare global` interface merging
// ---------------------------------------------------------------------------

#[test]
fn module_features_declare_global_merges_two_blocks() {
    // TS7 contract: `GlobalContractAlias = GlobalContract` (resolved through
    //   the global scope) must include the union of every property declared
    //   in every `declare global { interface GlobalContract { ... } }` block.
    //   Here that surface is `{ coreId: string; coreFlag: boolean }`.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_MAIN,
        "GlobalContractAlias",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["coreFlag", "coreId"]);
    assert_primitive(&props["coreId"].ty, PrimitiveName::String);
    assert_primitive(&props["coreFlag"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// `typeof import("./other-module")` projection
// ---------------------------------------------------------------------------

// LIFTED: `LeafDefault = LeafModule["default"]` where `LeafModule = typeof
// import("./module_features_leaf")` projects the default-export value shape
// `{ tag: "leaf-default"; count: number }` — `tag` const-narrowed to the literal
// (its initialiser value is `as const`, but the property is NOT `readonly`, since
// `leafDefault` itself is not `as const` — matching tsgo), `count` widened to
// `number`. The lifted body is the registry-keyed `oracle::run_row` shared-driver
// call comparing Verter's `Expanded` projection against the checked-in tsgo
// snapshot. Trace dispatches `ResolveDecl, Instantiate, IndexedAccess, TypeOf`,
// re-homing the row to `U2.INDEXED_ACCESS`.
#[oracle_row]
#[test]
fn module_features_typeof_import_default_resolves_value_shape() {}

#[test]
#[ignore = "MODULE_AUGMENTATION reducer complete: Verter resolves `import(\"./module_features_leaf\").LeafShape` to the declared `LeafShape` interface `{ id: string; count: number }` (verified). NOT oracle-liftable — the `import(\"…\").X` import-type source body lowers to a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"import-type\"))); lift pending an import-type source-walk carve-out"]
fn module_features_typeof_import_named_shape_resolves_to_interface() {
    // TS7 contract: `LeafNamedShape = import("./module_features_leaf").LeafShape`
    //   = `{ id: string; count: number }` (the declared `LeafShape` interface).
    //   `typeof import(...)` would not surface type-only exports — its value
    //   namespace excludes them. The `import(...)` form in type position
    //   exposes both type and value slots and is the canonical path for
    //   reaching a type-only export from a dynamic-import expression.
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "LeafNamedShape",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `LeafModule["leafName"]` where `export const leafName = "leaf"`
// reduces the named-value typeof-import index chain to the const-narrowed
// string literal `"leaf"`. The lifted body is the registry-keyed
// `oracle::run_row` shared-driver call comparing Verter's `Expanded`
// projection against the checked-in tsgo snapshot. Trace terminates at
// `IndexedAccess`, re-homing the row to `U2.INDEXED_ACCESS`.
#[oracle_row]
#[test]
fn module_features_typeof_import_named_value_resolves_to_literal() {}

// ---------------------------------------------------------------------------
// `declare module "./..."` interface augmentation merge
// ---------------------------------------------------------------------------

#[test]
fn module_features_module_augmentation_merges_plugin_surface() {
    // TS7 contract: `AugmentedPlugin = Plugin` (imported from
    //   `./module_features_base`) must surface every member contributed
    //   across all interface declarations:
    //     base   : `{ id: string }`
    //     patch  : `{ extra: number; label?: string }`
    //   The merged shape is `{ id: string; extra: number; label?: string }`.
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "AugmentedPlugin",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["extra", "id", "label"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["extra"].ty, PrimitiveName::Number);
    assert_primitive(&props["label"].ty, PrimitiveName::String);
    assert!(!props["id"].optional);
    assert!(!props["extra"].optional);
    assert!(props["label"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// `export = ` ambient module (CommonJS) interop
// ---------------------------------------------------------------------------

#[test]
#[ignore = "MODULE_AUGMENTATION reducer complete: Verter resolves `typeof import(\"./module_features_cjs\")` against the ambient `export = CjsCarrierValue` declaration to the `CjsCarrier` carrier `{ readonly tag: \"cjs\"; payload: number }` (verified). NOT oracle-liftable — the bare `typeof import(\"…\")` source body lowers to a deferred import-type construct (oracle admission Reject(DeferredConstruct(\"typeof-import\"))); lift pending a typeof-import source-walk carve-out"]
fn module_features_cjs_export_equals_resolves_to_carrier() {
    // TS7 contract: `CjsBinding = typeof import("./module_features_cjs")`.
    //   The ambient `module_features_cjs.d.ts` exports a single value via
    //   `export = CjsCarrierValue;` where `CjsCarrierValue: CjsCarrier`.
    //   So `typeof import(...)` reduces to the `CjsCarrier` interface =
    //   `{ readonly tag: "cjs"; payload: number }`.
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "CjsBinding",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["payload", "tag"]);
    assert_string_literal(&props["tag"].ty, "cjs");
    assert_primitive(&props["payload"].ty, PrimitiveName::Number);
    assert!(props["tag"].readonly);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Edge 1: `import { type SomeType, valueExport }` — type-only and value-only
//         specifiers on the same import statement.
// ---------------------------------------------------------------------------

#[test]
fn module_features_mixed_type_import_resolves_type_slot_to_leaf_shape() {
    // TS7 contract: `LeafTypeImported = LeafShape` (the type slot of a
    //   mixed `import { type LeafShape, leafName } from "./leaf"`) resolves
    //   to the declared interface shape `{ id: string; count: number }`.
    //   The `type` modifier on the specifier marks it as type-only at the
    //   import boundary but does NOT change the resolved type's structure.
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "LeafTypeImported",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn module_features_mixed_value_import_typeof_resolves_to_const_literal() {
    // TS7 contract: `LeafValueTypeof = typeof leafName` where the
    //   importer brings `leafName` in through the value slot of a mixed
    //   `import { type LeafShape, leafName } from "./leaf"`. The leaf
    //   declares `export const leafName = "leaf"` so the const-narrowed
    //   `typeof` resolves to the literal `"leaf"`.
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "LeafValueTypeof",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "leaf");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Edge 2: Namespace + interface name-merging.
// ---------------------------------------------------------------------------

#[test]
fn module_features_namespace_interface_merge_shape_resolves_to_interface() {
    // TS7 contract: When `interface Connector { id: string }` and
    //   `namespace Connector { ... }` declare the same name in the same
    //   scope, TypeScript merges them. The TYPE name `Connector` refers to
    //   the interface shape `{ id: string }`; namespace-qualified accesses
    //   like `Connector.Kind` reach into the namespace side. The aliases
    //   here exercise both halves.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_MAIN,
        "ConnectorShape",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn module_features_namespace_interface_merge_namespace_member_resolves() {
    // TS7 contract: `Connector.Kind` is a namespace-qualified type member
    //   that coexists with the merged `interface Connector`. It resolves
    //   to the string-literal union `"internal" | "external"`.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_MAIN,
        "ConnectorKind",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["external", "internal"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "MODULE_AUGMENTATION reducer complete: Verter resolves `typeof Connector.VERSION` through the merged interface+namespace declaration to the const-narrowed string literal `\"1.0\"` (verified). NOT oracle-liftable — the bare `typeof` source body is a deferred construct at the oracle source-walk (oracle admission Reject(DeferredConstruct(\"typeof\"))); lift pending a typeof-namespace-value carve-out"]
fn module_features_namespace_interface_merge_namespace_value_resolves_to_literal() {
    // TS7 contract: `typeof Connector.VERSION` where the merged namespace
    //   declares `export const VERSION = "1.0" as const` resolves to the
    //   literal string `"1.0"`.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_MAIN,
        "ConnectorVersion",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "1.0");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// Discriminating regression: namespace VALUE indexing is EXPORT-ONLY.
///
/// A `namespace N { ... }` only publishes `N.member` for members the
/// namespace `export`s; a non-exported `const hidden = …` is private to the
/// namespace body and `N.hidden` must NOT bind (TS: "Property 'hidden' does
/// not exist on type 'typeof N'").
///
/// Before the fix, the direct `Statement::VariableDeclaration` arm in
/// `extract_namespaced_statement` (type_eval_build.rs) and
/// `index_namespaced_statement` (decl_headers.rs) indexed EVERY namespaced
/// `const`/`let`/`var` under its qualified name — so the private `hidden` was
/// wrongly published as `N.hidden` and `typeof N.hidden` resolved to its value
/// (the number literal `1`). The fix removes the direct arm and keeps only the
/// `ExportNamedDeclaration -> VariableDeclaration` (exported) path.
///
/// Discrimination:
///  * POSITIVE — the EXPORTED `VERSION` still resolves (`typeof N.VERSION` →
///    `"1.0"`); proves the exported path is intact (same contract row 5 pins
///    for `Connector.VERSION`).
///  * NEGATIVE — the PRIVATE `hidden` no longer binds: `typeof N.hidden` misses
///    and projects to the `Unknown` carrier. This assertion FAILS pre-fix
///    (where `N.hidden` wrongly resolved to the literal `1`) and PASSES
///    post-fix. Verified red→green by stashing the analysis-crate change.
#[test]
fn module_features_namespace_value_indexing_is_export_only() {
    const PATH: &str = "/fixtures/namespace_export_only.ts";
    const SRC: &str = "export namespace N {\n  \
         const hidden = 1;\n  \
         export const VERSION = \"1.0\" as const;\n\
         }\n\
         export type V = typeof N.VERSION;\n\
         export type H = typeof N.hidden;\n";

    let host = make_host_with_footprint();
    upsert_ts(&host, PATH, SRC);

    // POSITIVE: the EXPORTED member resolves to its const-narrowed literal —
    // the export path that row 5 (`Connector.VERSION`) also depends on.
    let (v_expr, _) = resolve_expr(&host, PATH, "V", &[], ProjectionMode::Expanded);
    assert_string_literal(&v_expr, "1.0");

    // NEGATIVE / DISCRIMINATING: the PRIVATE (non-exported) `hidden` member must
    // NOT bind as `N.hidden`; `typeof N.hidden` misses → `Unknown` carrier.
    // Pre-fix the over-broad direct arm indexed `N.hidden`, so this resolved to
    // the number literal `1` (not `Unknown`).
    let (h_expr, _) = resolve_expr(&host, PATH, "H", &[], ProjectionMode::Expanded);
    assert!(
        matches!(h_expr, TypeExpr::Unknown { .. }),
        "`typeof N.hidden` (a PRIVATE, non-exported namespace local) must NOT resolve \
         to the private value — namespace value indexing is export-only; got {h_expr:?}",
    );
}

// ---------------------------------------------------------------------------
// Edge 3: `declare module "external-spec"` string-literal module
//         augmentation merge.
// ---------------------------------------------------------------------------

#[test]
fn module_features_external_module_augmentation_merges_config() {
    // TS7 contract: `module_features_external.d.ts` declares
    //   `declare module "external-spec" { interface Config { base: string } }`
    //   and `module_features_external_patch.ts` adds
    //   `declare module "external-spec" { interface Config { extra: number } }`.
    //   After both files are loaded, a consumer importing `Config` from
    //   `"external-spec"` sees the merged interface
    //   `{ base: string; extra: number }`. This is the canonical
    //   Vue/Vite `vite/client` augmentation pattern.
    let host = make_host_with_footprint();
    upsert_external_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_EXTERNAL_CONSUMER,
        "ExternalConfig",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["base", "extra"]);
    assert_primitive(&props["base"].ty, PrimitiveName::String);
    assert_primitive(&props["extra"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
