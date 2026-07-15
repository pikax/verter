// Vendored fixture source bytes for the U2.MODULE_AUGMENTATION-era oracle
// lift rows (`modern_ts_features` / `module_features` + its 5-file
// consumer graph), split out of `oracle_query_specs_vendored_sources.rs` to
// keep each vendored-sources file under the production line-size guard.
// `include!`'d by `oracle_query_specs.rs` immediately after the primary
// vendored-sources file (the registry is the source-byte authority; the
// guard `inlined_registry_source_is_byte_identical_to_fixture_files` pins
// each const byte-identical to its on-disk `fixtures/*.ts` sibling).

// ── Module-augmentation lift sources (Part BC). Vendored byte-identical to the
//    on-disk fixtures; the guard `inlined_registry_source_is_byte_identical_to_fixture_files`
//    pins each const equal to its `fixtures/*.ts` sibling.

/// Vendored source bytes of `/fixtures/modern_ts_features.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/modern_ts_features.ts`.
#[allow(dead_code)]
pub(crate) const MODERN_TS_FEATURES_SOURCE: &str = r#"// @ai-generated - Synthetic modern-TS-features typeinfo fixture.
//
// Covers TS7 feature surface that NoInfer and `const T` don't already
// characterise:
//
//   * Variance annotations `<in T>`, `<out U>`, `<in out V>` — used by
//     interface declarations. Variance is enforced by assignability, not by
//     the structural shape; the projector should still surface the declared
//     members.
//
//   * `using` declarations + `Symbol.dispose` — encoded structurally as a
//     function that returns the consumed value. The fixture declares its own
//     `DisposableLike` interface that mirrors the lib.esnext.disposable shape
//     so the surface is hermetic and does not rely on the runtime lib.
//
//   * `await using` + `Symbol.asyncDispose` — analogous to `using`, with an
//     async dispose return.
//
//   * Import attributes `import x from "..." with { type: "json" }` — encoded
//     structurally as an `as const` literal so the surface characterises what
//     an import-attribute'd JSON module would produce.
//
//   * `satisfies` operator deep widening: literal keys are preserved, but
//     inner values widen unless `as const` is applied. This captures the
//     `satisfies` ! = `as const` distinction.
//
// Where tsgo (7.0.0-dev.20260423.1) cannot or might not typecheck a true
// in-language form of the feature (real `using`, real import attributes), the
// fixture uses a SIMULATED structural encoding that produces the same surface
// the real feature would yield. Each simulated path is documented at its
// declaration so the test contract stays unambiguous.

// ---------------------------------------------------------------------------
// (1) Variance annotations
// ---------------------------------------------------------------------------
// `<out T>` — covariant; `<in T>` — contravariant; `<in out T>` — invariant.
// These are checked at assignability time, not in the structural surface.
// The projector should publish the interface members as declared.

export interface Producer<out T> {
  create(): T;
}
export interface Consumer<in T> {
  consume(value: T): void;
}
export interface Invariant<in out T> {
  transfer(value: T): T;
}

export type ProducerString = Producer<string>;
export type ConsumerNumber = Consumer<number>;
export type InvariantBoolean = Invariant<boolean>;

// ---------------------------------------------------------------------------
// (2) `using` declarations / Symbol.dispose (SIMULATED structural form)
// ---------------------------------------------------------------------------
// The real `using` lexical form requires lib.esnext.disposable in scope.
// To keep this fixture hermetic, we declare our own DisposableLike interface
// with the same shape as the lib emission and write a normal helper that
// consumes a DisposableLike and returns its `.value` field. The structural
// SURFACE the test characterises is identical to the surface that
// `using resource = makeResource(); return resource.value` would produce: the
// helper's return type is `string`.
//
// If tsgo gains real `using` support and we want a literal in-language test,
// replace `consumeDisposable` with a function whose body is:
//   using resource = makeResource();
//   return resource.value;
// — the return type characterisation does not change.

export interface DisposableLike {
  readonly value: string;
  // The real `using` keyword requires the operand to satisfy the
  // built-in `Disposable` interface which carries `[Symbol.dispose](): void`.
  // We mirror that exact key here so the `RealUsingResult` companion test
  // below typechecks against tsgo's `using` semantics. The simulated
  // `consumeDisposable` test that consumes this type via a regular `const`
  // is unaffected by the additional key.
  [Symbol.dispose](): void;
}

export declare function makeDisposable(): DisposableLike;
export function consumeDisposable(): string {
  const resource = makeDisposable();
  try {
    return resource.value;
  } finally {
    resource[Symbol.dispose]();
  }
}
export type ConsumeDisposableResult = ReturnType<typeof consumeDisposable>;

// ---------------------------------------------------------------------------
// (3) `await using` / Symbol.asyncDispose (SIMULATED structural form)
// ---------------------------------------------------------------------------
// Same hermeticity rationale as (2): declare our own AsyncDisposableLike with
// the dispose method renamed to `disposeAsync` (mirroring the runtime
// `[Symbol.asyncDispose]` slot) and write a normal async helper. The
// resolved surface for `consumeAsyncDisposable` is `Promise<number>`; the
// resolved type of `AsyncConsumeResult` is `number` after unwrap.

export interface AsyncDisposableLike {
  readonly count: number;
  disposeAsync(): Promise<void>;
}

export declare function makeAsyncDisposable(): AsyncDisposableLike;
export async function consumeAsyncDisposable(): Promise<number> {
  const resource = makeAsyncDisposable();
  try {
    return resource.count;
  } finally {
    await resource.disposeAsync();
  }
}
export type AsyncConsumeResult = Awaited<ReturnType<typeof consumeAsyncDisposable>>;

// ---------------------------------------------------------------------------
// (4) Import attributes (SIMULATED form)
// ---------------------------------------------------------------------------
// Real form:
//   import data from "./config.json" with { type: "json" };
//   type ConfigData = typeof data;
//
// tsgo may not resolve the JSON module without a real ./config.json on disk;
// we declare the imported shape inline as an `as const` literal. The
// resolved surface is identical to the real form: the imported JSON value
// would yield the same literal object shape if the JSON file held those
// exact values.

const importedJsonConfig = {
  name: "verter-fixture",
  version: 1,
} as const;
export type ImportedJsonConfig = typeof importedJsonConfig;
export type ImportedJsonName = ImportedJsonConfig["name"];

// ---------------------------------------------------------------------------
// (5) `satisfies` operator — deep widening behaviour
// ---------------------------------------------------------------------------
// `satisfies` constrains a value against a target type WITHOUT widening the
// value's type to the target. Object keys are preserved as literal keys
// (because the object expression keeps its inferred shape), but inner values
// widen UNLESS the value is `as const`-asserted. This means:
//   * `keyof typeof cfg` preserves the literal keys "a" | "b"
//   * `typeof cfg.a.count` widens to `number` (NOT the literal `1`)

export type CfgEntry = { count: number };
export type CfgShape = Record<string, CfgEntry>;
export const cfg = {
  a: { count: 1 },
  b: { count: 2 },
} satisfies CfgShape;

export type CfgKeys = keyof typeof cfg;
export type CfgValueACount = typeof cfg.a.count;

// ---------------------------------------------------------------------------
// (6) Variance annotation T substitution through Consumer.consume
// ---------------------------------------------------------------------------
// The existing variance tests prove the interface declaration is resolved
// structurally. This case exercises the type-parameter SUBSTITUTION through a
// method member: `Parameters<NumberConsumer["consume"]>` must materialise the
// labelled tuple `[value: number]` — T must be substituted from the
// variance-annotated `<in T>` parameter into the consume method's parameter,
// NOT left as the generic `T`.

export type NumberConsumer = Consumer<number>;
export type NumberConsumerParameters = Parameters<NumberConsumer["consume"]>;

// ---------------------------------------------------------------------------
// (7) `satisfies` with array literal
// ---------------------------------------------------------------------------
// TS7 contract: `typeof arrSat` resolves to `number[]`. The `satisfies
// readonly number[]` clause checks assignability but does NOT preserve the
// tuple shape — the inferred type for `[1, 2, 3]` (without `as const`) is
// `number[]`. This locks in the documented `satisfies` != `as const`
// behaviour for array literals.

export const arrSat = [1, 2, 3] satisfies readonly number[];
export type ArrSatType = typeof arrSat;

// ---------------------------------------------------------------------------
// (8) Real `using` declaration
// ---------------------------------------------------------------------------
// Companion to the existing simulated `using` form above. Exercises the real
// `using` keyword against the same DisposableLike shape. The structural
// return surface is identical to `consumeDisposable`: the helper's return
// type is `string`. Captured separately as `RealUsingResult` so the test
// contract states unambiguously what is being characterised.

export function consumeDisposableUsing(): string {
  using resource = makeDisposable();
  return resource.value;
}
export type RealUsingResult = ReturnType<typeof consumeDisposableUsing>;
"#;

/// Vendored source bytes of `/fixtures/module_features.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/module_features.ts`.
#[allow(dead_code)]
pub(crate) const MODULE_FEATURES_SOURCE: &str = r#"// @ai-generated - Synthetic module-feature typeinfo fixture.
//
// Covers:
//   * Single-level `namespace Geometry { ... }` member resolution.
//   * Nested `namespace A.B.C { ... }` deep member resolution.
//   * `declare global { interface GlobalContract { ... } }` augmenting a
//     locally-declared global interface.
//
// Module augmentation (`declare module "./..."`), `typeof import("./...")`,
// and `export = ` ambient module interop live in companion fixtures
// because they need separate canonical files.

// Force this file to be a module so `declare global` is valid.
export {};

declare global {
  // A locally-declared global contract that this file augments.
  // Tests resolve `GlobalContract` from any file in the project — the
  // augmented surface includes properties contributed here.
  interface GlobalContract {
    coreId: string;
  }
}

declare global {
  interface GlobalContract {
    coreFlag: boolean;
  }
}

// Single-level namespace. `Geometry.Point` is the canonical example.
export namespace Geometry {
  export type Point = { x: number; y: number };
  export type Vector = Point;
}

// Nested namespace `A.B.C` — deep member resolution must walk the chain.
export namespace Layer {
  export namespace Inner {
    export namespace Leaf {
      export type Value = { tag: "leaf"; depth: number };
    }
  }
}

// Aliases that consumers will resolve through. These exercise the
// "resolve by alias name" path against namespace-qualified definitions.
export type GeometryPoint = Geometry.Point;
export type GeometryVector = Geometry.Vector;
export type LeafValue = Layer.Inner.Leaf.Value;

// `GlobalContractAlias` projects the global interface via a local alias
// so the test can request it by a stable resolver-symbol name.
export type GlobalContractAlias = GlobalContract;

// Namespace + interface name-merging. `Connector` is BOTH:
//   * an interface (the type) → `{ id: string }`
//   * a namespace (the value/type container) exposing `Kind` and `VERSION`
// In TS7 the two declarations merge: `Connector` as a type refers to the
// interface shape, while `Connector.Kind` / `Connector.VERSION` reach into
// the namespace.
export interface Connector {
  id: string;
}
export namespace Connector {
  export type Kind = "internal" | "external";
  export const VERSION = "1.0" as const;
}

// Aliases the resolver requests by name.
export type ConnectorShape = Connector;
export type ConnectorKind = Connector.Kind;
export type ConnectorVersion = typeof Connector.VERSION;
"#;

/// Vendored source bytes of `/fixtures/module_features_leaf.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/module_features_leaf.ts`.
#[allow(dead_code)]
pub(crate) const MODULE_FEATURES_LEAF_SOURCE: &str = r#"// @ai-generated - Synthetic leaf for `typeof import("./...")` typeinfo
// tests. Exports both a default value and named values so the consumer
// can probe `.default` and named-export shapes.

export const leafName = "leaf";
export interface LeafShape {
  id: string;
  count: number;
}
export function leafFactory(): LeafShape {
  return { id: leafName, count: 0 };
}

const leafDefault = { tag: "leaf-default" as const, count: 0 };
export default leafDefault;
"#;

/// Vendored source bytes of `/fixtures/module_features_base.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/module_features_base.ts`.
#[allow(dead_code)]
pub(crate) const MODULE_FEATURES_BASE_SOURCE: &str = r#"// @ai-generated - Synthetic base module for `declare module "./..."`
// interface-merging augmentation tests.

export interface Plugin {
  id: string;
}

export function makePlugin(): Plugin {
  return { id: "base" } as Plugin;
}
"#;

/// Vendored source bytes of `/fixtures/module_features_patch.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/module_features_patch.ts`.
#[allow(dead_code)]
pub(crate) const MODULE_FEATURES_PATCH_SOURCE: &str = r#"// @ai-generated - Synthetic patch module that augments
// `module_features_base.ts` via `declare module "./..."` interface
// merging.

import type { Plugin } from "./module_features_base";
import "./module_features_base";

declare module "./module_features_base" {
  interface Plugin {
    extra: number;
    label?: string;
  }
}

export function describePlugin(plugin: Plugin): string {
  return `${plugin.id}:${plugin.extra}:${plugin.label ?? "(no label)"}`;
}
"#;

/// Vendored source bytes of `/fixtures/module_features_cjs.d.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/module_features_cjs.d.ts`.
#[allow(dead_code)]
pub(crate) const MODULE_FEATURES_CJS_SOURCE: &str = r#"// @ai-generated - Synthetic CommonJS-style `export = ` ambient module
// for the `import M = require("./...")` typeinfo interop test. Lives in
// `.d.ts` because `export = ` is only allowed in declaration files (or
// with CommonJS module targets) — the consumer compiles under
// `--module esnext`.

interface CjsCarrier {
  readonly tag: "cjs";
  payload: number;
}

declare const CjsCarrierValue: CjsCarrier;
export = CjsCarrierValue;
"#;

/// Vendored source bytes of `/fixtures/module_features_consumer.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/module_features_consumer.ts`.
#[allow(dead_code)]
pub(crate) const MODULE_FEATURES_CONSUMER_SOURCE: &str = r#"// @ai-generated - Synthetic consumer that ties together
//   * `typeof import("./module_features_leaf")` named + default exports,
//   * `declare module "./module_features_base"` augmented surface,
//   * `typeof import("./module_features_cjs")` CommonJS-export-= interop.

import type { Plugin } from "./module_features_base";
import "./module_features_patch";

// `typeof import("./mod")` returns the module's runtime VALUE namespace;
// it does NOT include type-only exports. Type-only exports must be
// reached through the dynamic-import-in-type-position form
// `import("./mod").TypeName` (which exposes both type and value slots,
// and which is itself a valid stored type alias). Both forms are used
// below to exercise the value-namespace path AND the type-slot path
// against the same leaf module.
export type LeafModule = typeof import("./module_features_leaf");
export type LeafDefault = LeafModule["default"];
export type LeafNamedShape = import("./module_features_leaf").LeafShape;
export type LeafNamedValue = LeafModule["leafName"];

// The augmented `Plugin` surface — must include base `id`, patch `extra`,
// patch `label?`.
export type AugmentedPlugin = Plugin;

// `typeof import("./module_features_cjs")` against an `export = ` module
// gives the type of the export-= value directly.
export type CjsBinding = typeof import("./module_features_cjs");

// Mixed `import { type X, valueY }` syntax against `module_features_leaf`.
// `LeafShape` is a type-only specifier; `leafName` is a value-only
// specifier. The two slots resolve independently:
//   * `LeafShape` → the declared interface shape
//   * `typeof leafName` → the literal type `"leaf"` (`const`-narrowed)
import { type LeafShape, leafName } from "./module_features_leaf";

export type LeafTypeImported = LeafShape;
export type LeafValueTypeof = typeof leafName;
"#;

/// The workspace-file set the `modern_ts_features.rs` import-attribute lift row upserts.
#[allow(dead_code)]
const MODERN_TS_FEATURES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/modern_ts_features.ts",
    source: MODERN_TS_FEATURES_SOURCE,
}];

/// The single-file workspace the `module_features.rs` main-file lift row upserts
/// (the namespace alias-chain row resolves against the standalone main fixture).
#[allow(dead_code)]
const MODULE_FEATURES_MAIN_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/module_features.ts",
    source: MODULE_FEATURES_SOURCE,
}];

/// The 5-file consumer-graph workspace the `module_features.rs` typeof-import lift
/// rows upsert (`upsert_consumer_graph`): leaf + base + patch + cjs + consumer; the
/// primary canonical is `/fixtures/module_features_consumer.ts`.
#[allow(dead_code)]
const MODULE_FEATURES_CONSUMER_FILES: &[WorkspaceFileSpec] = &[
    WorkspaceFileSpec {
        path: "/fixtures/module_features_leaf.ts",
        source: MODULE_FEATURES_LEAF_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/module_features_base.ts",
        source: MODULE_FEATURES_BASE_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/module_features_patch.ts",
        source: MODULE_FEATURES_PATCH_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/module_features_cjs.d.ts",
        source: MODULE_FEATURES_CJS_SOURCE,
    },
    WorkspaceFileSpec {
        path: "/fixtures/module_features_consumer.ts",
        source: MODULE_FEATURES_CONSUMER_SOURCE,
    },
];
