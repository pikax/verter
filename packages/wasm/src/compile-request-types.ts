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
// RESPONSE side — `@verter/native` has a counterpart (`HostCompileResponse`
// / `HostCompiledProduct`), but the JavaScript envelopes diverge: native
// nests `ide`, stringifies `analysis`, and throws a structured Error; this
// binding flattens the IDE DTO, returns `analysis` as an object, has no
// `null` on optional fields, and throws a string. Names here stay
// unprefixed. Every response declaration is RENDERED from the Rust types
// this binding serialises (`compile-response.generated.ts`, byte-pinned
// against them) and re-exported below; the shared leaf shapes it reaches
// are imported from `@verter/native`'s host types, never restated.
//
// They live in a leaf module rather than in `index.ts` so `tsc` can check
// them: `index.ts` imports the gitignored wasm-bindgen artifact and cannot
// be type-checked from a clean tree.
// =============================================================================

import type {
  HostAnalysisProductOptions,
  HostCompileIdentity,
  HostIdeProductOptions,
  HostRuntimeProductOptions,
  HostSvelteCompileOptions,
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

export type {
  HostCompiledVirtualNode,
  HostCompiledRuntimeProduct,
  HostCompiledIdeProduct,
  HostTemplateAnalysisSnapshot,
  HostCompiledAnalysisProduct,
  HostCompiledProduct,
  HostCompileRequestResponse,
} from "./compile-response.generated";

/**
 * Where a `<script setup>` destructured-props block sits in the generated
 * IDE surface, and which bindings it declares: the shared declaration the
 * IDE row's `destructuredBlock` carries.
 */
export type { HostDestructuredBlockMeta } from "@verter/native/host-types";
