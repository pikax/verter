// =============================================================================
// The typed compile request and its response
//
// REQUEST side — the arm name is the object's single key (`{ vue: … }`,
// `{ analysis: … }`), where the native binding's decoder reads an internal
// `framework` / `kind` tag. That difference is why every request type here
// carries a `Browser` prefix: `@verter/native` exports `HostCompileRequest`
// and `HostRequestedProduct` as the internally-tagged forms, and a shared
// name would let a payload move between the two packages, keep
// type-checking, and be refused by the other decoder at run time.
//
// Only the tagged wrappers are declared here — every leaf option, identity
// and product shape is imported from `@verter/native`'s generated
// projection, which is rendered from the very declarations this binding
// decodes and byte-pinned against them. The arm SETS and arm PAYLOADS are
// held to that projection in `src/index.test-d.ts`, so a product or
// framework arm added, removed or renamed on the shared schema is a compile
// error there rather than a shape that type-checks and then refuses at run
// time.
//
// RESPONSE side — no native counterpart exists, so those names are
// unprefixed. They reuse the shapes this package already re-exports from
// `@verter/native` wherever the route serialises one, rather than restating
// them: a second copy is only safe when something holds the two together,
// and these have the strongest possible tie — the route serialises the very
// Rust struct those declarations project.
//
// They live in a leaf module rather than in `index.ts` so `tsc` can check
// them: `index.ts` imports the gitignored wasm-bindgen artifact and cannot
// be type-checked from a clean tree.
// =============================================================================

import type {
  HostAnalysisProductOptions,
  HostCompileIdentity,
  HostDiagnosticsSnapshot,
  HostIdeProductOptions,
  HostIdeResponse,
  HostRuntimeProductOptions,
  HostSvelteCompileOptions,
  HostVirtualMeta,
  HostVirtualNodeKind,
  HostVueCompileOptions,
} from "@verter/native/host-types";

export type {
  HostAnalysisProductOptions,
  HostCompileIdentity,
  HostIdeProductOptions,
  HostRuntimeProductOptions,
  HostSvelteCompileOptions,
  HostVueCompileOptions,
} from "@verter/native/host-types";

/**
 * A union whose populated arms are mutually exclusive at the type level.
 *
 * TypeScript's excess-property check does not fire through a plain union:
 * an object stating BOTH `vue` and `svelte` satisfies
 * `{ vue: … } | { svelte: … }` with no diagnostic, while the decoder
 * refuses it as an unknown field. A well-typed call would then be a
 * guaranteed run-time throw. Each arm therefore declares every OTHER arm's
 * key as an optional `never`, so a multi-arm payload is a compile error
 * exactly where the decoder would have refused it. Under TypeScript's
 * default optional-property semantics, an explicitly written `undefined`
 * still satisfies an optional `never`; the callable route treats those
 * known undefined sibling tags as absent at both tagged-union layers.
 */
type ExactlyOneOf<Arms> = {
  [Arm in keyof Arms]: Pick<Arms, Arm> & Partial<Record<Exclude<keyof Arms, Arm>, never>>;
}[keyof Arms];

/**
 * One requested compiler product. There is no target preset that expands
 * into a bundle, and request order is preserved in the response.
 *
 * `compileRequest()` currently produces `runtimeClient`, `runtimeServer`,
 * `ideCompanion`, and `analysis`. The shared schema also contains
 * `publicApi` and `declarations`, but this browser route cannot produce
 * either and throws their refusal message as a string.
 *
 * An arm that carries no options is the bare tag STRING, not an object:
 * `"publicApi"`, never `{ publicApi: {} }`. The option-carrying arms are
 * mutually exclusive — one product row states one product.
 */
export type BrowserHostRequestedProduct =
  | ExactlyOneOf<{
      runtimeClient: HostRuntimeProductOptions;
      runtimeServer: HostRuntimeProductOptions;
      ideCompanion: HostIdeProductOptions;
      analysis: HostAnalysisProductOptions;
    }>
  | "publicApi"
  | "declarations";

export interface BrowserHostVueCompileRequest {
  identity: HostCompileIdentity;
  products: BrowserHostRequestedProduct[];
  options: HostVueCompileOptions;
}

export interface BrowserHostSvelteCompileRequest {
  identity: HostCompileIdentity;
  products: BrowserHostRequestedProduct[];
  options: HostSvelteCompileOptions;
}

/**
 * A host compile request discriminated by framework at the outermost level,
 * so framework-owned options are structurally unreachable from the other
 * framework's arm and a foreign key inside either arm is refused at decode.
 *
 * Exactly one populated framework arm: naming both is a compile error here
 * and a decoder refusal. An explicitly undefined sibling is treated as
 * absent, matching TypeScript's default optional-property semantics.
 */
export type BrowserHostCompileRequest = ExactlyOneOf<{
  vue: BrowserHostVueCompileRequest;
  svelte: BrowserHostSvelteCompileRequest;
}>;

// -----------------------------------------------------------------------------
// The response
// -----------------------------------------------------------------------------

/**
 * One separately addressed output of a compiled runtime product.
 *
 * A runtime product is not one blob: the assembled main module, the script,
 * the compiled template, each style block and each custom block are distinct
 * modules a consumer loads and maps independently, so each row carries its
 * own code and its own map.
 *
 * The optional fields carry no `| null`: the route serialises through
 * `serde_wasm_bindgen`'s default serializer, where an absent value is
 * `undefined` and never `null`.
 */
export interface HostCompiledVirtualNode {
  /** Which node of the carrier this row is. */
  node: HostVirtualNodeKind;
  code: string;
  /** This node's own JSON source map, when the request asked for maps. */
  sourceMap?: string;
  /** Output language (e.g. `"js"`, `"ts"`, `"css"`). */
  lang?: string;
  /** Block-specific metadata carried beside the node's bytes. */
  meta: HostVirtualMeta;
}

export interface HostCompiledRuntimeProduct {
  kind: "runtimeClient" | "runtimeServer";
  nodes: HostCompiledVirtualNode[];
}

/**
 * Where a `<script setup>` destructured-props block sits in the generated
 * IDE surface, and which bindings it declares.
 *
 * Declared here because the shared `HostIdeResponse` does not carry it,
 * while every binding that publishes an IDE projection does. Offsets are
 * UTF-16 code units.
 */
export interface HostDestructuredBlockMeta {
  /** Binding spans in UTF-16 code units into the registered source. */
  bindings: { name: string; sourceStart: number; sourceEnd: number }[];
  /** UTF-16 start offset into the IDE product row's generated `code`. */
  blockStart: number;
  /** UTF-16 end offset into the IDE product row's generated `code`. */
  blockEnd: number;
}

/**
 * The IDE projection row: the shared IDE response plus the destructured
 * block the shared declaration does not name.
 */
export type HostCompiledIdeProduct = { kind: "ideCompanion" } & HostIdeResponse & {
    destructuredBlock?: HostDestructuredBlockMeta;
  };

/**
 * The TEMPLATE analysis snapshot — binding occurrences, expression
 * diagnostics, and the rest of the template's facts.
 *
 * This is NOT what `getAnalysis()` returns. `getAnalysis()` publishes the
 * whole-file snapshot (imports, bindings, macros, styles, and a nested
 * `template`); this payload is that snapshot's `template` value and nothing
 * else. `@verter/language-shared` declares the same object as
 * `TemplateAnalysisSnapshot`; it stays unstructured here rather than
 * pulling an editor package into a browser binding's published types, and
 * the equivalence is proven at run time by the browser boundary tests
 * rather than asserted only in prose.
 *
 * Its spans are UTF-8 BYTE offsets into the SFC source. Diagnostics and
 * destructured binding source spans are UTF-16 offsets into that source;
 * destructured block bounds are UTF-16 offsets into the generated IDE code.
 */
export type HostTemplateAnalysisSnapshot = Record<string, unknown>;

export interface HostCompiledAnalysisProduct {
  kind: "analysis";
  /**
   * Nested under its own key rather than flattened into the row, so no
   * field of the payload can collide with the `kind` discriminant.
   */
  analysis: HostTemplateAnalysisSnapshot;
}

/**
 * One row of a response's product list, one-to-one with a requested product
 * kind and in request order, tagged with the same spelling the request used.
 */
export type HostCompiledProduct =
  | HostCompiledRuntimeProduct
  | HostCompiledIdeProduct
  | HostCompiledAnalysisProduct;

/**
 * The result of executing one typed compile request.
 *
 * Complete-only: every requested product is present. A refusal at any stage
 * THROWS — there is no partial response, no `null`, and no boolean.
 */
export interface HostCompileRequestResponse {
  /** The canonical id the request executed against, after alias resolution. */
  canonicalId: string;
  /**
   * The compile's diagnostics, deduplicated once across every product.
   * Spans are UTF-16 offsets into the registered source.
   */
  diagnostics: HostDiagnosticsSnapshot;
  /** One row per requested product kind, in request order. */
  products: HostCompiledProduct[];
}
