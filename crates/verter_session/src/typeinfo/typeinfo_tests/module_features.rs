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

use super::support::*;
use crate::VerterHost;

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

#[test]
#[ignore = "typeinfo currently does not project a namespace alias chain like `Geometry.Vector` (which aliases `Geometry.Point`) to the underlying object shape; keep as the future namespace alias chain contract"]
fn module_features_namespace_geometry_vector_aliases_point() {
    // TS7 contract: `GeometryVector = Geometry.Vector = Geometry.Point` =
    //   `{ x: number; y: number }`. Namespace-qualified alias chains
    //   must collapse to the final shape.
    let host = make_host_with_footprint();
    upsert_main(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_MAIN,
        "GeometryVector",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["x", "y"]);
    assert_primitive(&props["x"].ty, PrimitiveName::Number);
    assert_primitive(&props["y"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

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
#[ignore = "typeinfo currently does not merge multiple `declare global { interface ... }` blocks into a single resolved global interface surface; keep as the future declare-global merge contract"]
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

#[test]
#[ignore = "typeinfo currently does not project `typeof import('./module')['default']` to the type of the default export value; keep as the future typeof-import default-export contract"]
fn module_features_typeof_import_default_resolves_value_shape() {
    // TS7 contract: `LeafDefault = LeafModule["default"]` where
    //   `LeafModule = typeof import("./module_features_leaf")`. The default
    //   export is `{ tag: "leaf-default" as const; count: 0 }` — under
    //   `const` inference `tag` is the literal `"leaf-default"` and
    //   `count` is widened to `number` (the rhs `0` is not `as const`).
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "LeafDefault",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "tag"]);
    assert_string_literal(&props["tag"].ty, "leaf-default");
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project `import('./module')['NamedShape']` to the named-export type's declared shape; keep as the future dynamic-import named-export-shape contract"]
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

#[test]
#[ignore = "typeinfo currently does not project `typeof import('./module')['namedValue']` to the type of the named-value export; keep as the future typeof-import named-value contract"]
fn module_features_typeof_import_named_value_resolves_to_literal() {
    // TS7 contract: `LeafNamedValue = LeafModule["leafName"]` where
    //   `export const leafName = "leaf"` — `typeof leafName` is the string
    //   literal `"leaf"` (const-narrowed).
    let host = make_host_with_footprint();
    upsert_consumer_graph(&host);

    let (expr, record) = resolve_expr(
        &host,
        PATH_CONSUMER,
        "LeafNamedValue",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "leaf");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// `declare module "./..."` interface augmentation merge
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not discover module augmentations contributed by a sibling file pulled in via a side-effect `import \"./patch\";` — the consumer sees only the base interface members. Keep as the future side-effect-imported-augmentation discovery contract"]
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
#[ignore = "typeinfo currently does not project `typeof import('./cjs')` against an ambient `export = ` declaration to the export-= value type; keep as the future export-equals interop contract"]
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
#[ignore = "typeinfo currently does not project `typeof Connector.VERSION` (a namespace-qualified const value with `as const` narrowing) through a merged interface+namespace declaration; keep as the future merged-namespace value-member contract"]
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

// ---------------------------------------------------------------------------
// Edge 3: `declare module "external-spec"` string-literal module
//         augmentation merge.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "typeinfo currently does not merge `declare module \"external-spec\" { interface Config { ... } }` blocks across files (the canonical Vite/Vue `vite/client` augmentation pattern); keep as the future string-literal module-augmentation contract"]
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
