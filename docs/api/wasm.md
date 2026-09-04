# @verter/wasm

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

WASM bindings for browser-based SFC compilation. Powers the online playground.

## Installation

```bash
pnpm add @verter/wasm
```

## API

### `initialize()`

Load the WASM module. Must be called before constructing a `Host` (`createHost()` does it for you). Safe to call multiple times -- only initializes once. Subsequent calls return immediately.

```ts
import { initialize } from "@verter/wasm";

await initialize();
```

### `isInitialized()`

Check if the WASM module has been loaded.

```ts
import { isInitialized } from "@verter/wasm";

if (!isInitialized()) {
  await initialize();
}
```

### Standalone `compile()` / `compileSync()` — removed

`@verter/wasm` has no standalone compile function. The WASM artifact
exports `VerterHost` and no free compile entry, so the wrappers that
claimed to offer one always threw; they were removed rather than left
in place. Compile through [`Host` / `VerterHost`](#host--verterhost).

### `Host` / `VerterHost`

In-memory host facade exposed by the WASM runtime. It shares the typed
`compileRequest()` and profile-bearing read methods with `@verter/native`, but
the native-only source-registering `compileRequests()` batch route is absent.

```ts
import { createHost } from "@verter/wasm";

const host = await createHost({ devMode: true });

const update = host.upsert({
  inputId: "App.vue",
  source: sfcSource,
});

// update.moduleReferences — import/require sites for dependency tracking

const file = host.getVirtualFile({
  rawId: "App.vue",
  compileProfile: { isProduction: false },
});
```

#### `createHost(config?)`

Async factory that initializes WASM (if needed) and returns a new `Host` instance.

```ts
import { createHost } from "@verter/wasm";

const host = await createHost();
```

**Returns:** `Promise<Host>`

#### Host Methods

The `Host` class exposes the shared host methods below, including
`compileRequest()`:

| Method                                                                                      | Returns                           | Description                                                     |
| ------------------------------------------------------------------------------------------- | --------------------------------- | --------------------------------------------------------------- |
| `resolve(rawId)`                                                                            | `HostResolvedId \| null`          | Resolve raw ID to canonical ID                                  |
| `upsert(request)`                                                                           | `HostUpdateResult`                | Register/update a file                                          |
| `applyBlockOverrides(request)`                                                              | `HostUpdateResult`                | Apply preprocessed block overrides                              |
| `getIde(canonicalId, profile?)`                                                             | `HostIdeResponse \| null`         | Get TSX or JSX for type checking                                |
| `compileRequest(canonicalId, request)`                                                      | `HostCompileRequestResponse`      | Execute one typed compile request (throws on refusal)           |
| `getVirtualFile(query)`                                                                     | `HostVirtualFileResponse \| null` | Get compiled virtual file (`null` when the node does not exist) |
| `listVirtualFiles(canonicalId)`                                                             | `HostVirtualNodeKind[]`           | List virtual nodes for a file                                   |
| `remove(canonicalOrAlias)`                                                                  | `HostRemoveResult \| null`        | Remove file from host                                           |
| `getAnalysis(canonicalOrAlias)`                                                             | `unknown \| null`                 | Get analysis snapshot (native JS object)                        |
| `setImportDependencies(id, deps)`                                                           | `void`                            | Set resolved import dependencies                                |
| `collectResolvableModuleReferenceSpecifiers(moduleReferences)`                              | `string[]`                        | Return exact/finite candidate specifiers in encounter order     |
| `resolveKnownModuleReferenceDependencies(ownerId, moduleReferences, knownIds, extensions?)` | `string[]`                        | Resolve exact/finite candidates against an in-memory file set   |

See the [@verter/native documentation](./native.md) for detailed descriptions of each method and their parameter types.

`applyBlockOverrides` uses sealed block identity, never a style index. Echo the
stamps from the `preprocessorRequests` entry and hash the exact bytes returned by
the processor:

```ts
async function hashBlockContent(value: string): Promise<string> {
  const prefix = new TextEncoder().encode("verter.block-content.bytes.v1\0");
  const bytes = new TextEncoder().encode(value);
  const input = new Uint8Array(prefix.length + bytes.length);
  input.set(prefix);
  input.set(bytes, prefix.length);
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", input));
  return `sha256:${Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

const update = host.upsert({
  inputId: "/src/App.vue",
  source: '<template lang="pug">p Hello</template>',
});
const pending = update.preprocessorRequests[0];
const code = "<p>Hello</p>";

host.applyBlockOverrides({
  canonicalId: update.canonicalId,
  overrides: [
    {
      correlationToken: pending.correlationToken,
      blockToken: pending.blockToken,
      ownerRevision: pending.ownerRevision,
      artifactToken: pending.artifactToken,
      basisToken: pending.basisToken,
      sourceSpaceToken: pending.sourceSpaceToken,
      code,
      codeHash: await hashBlockContent(code),
    },
  ],
});
```

The host validates the current revision, artifact, basis, source space, code
hash, and optional source-map hash after the asynchronous processor completes.
It refuses stale or mismatched results without mutating its caches.

#### `compileRequest(canonicalId, request)`

Executes one typed compile request against an already-registered source. The
whole transaction is one call: register the carrier once with `upsert()`,
source only, then hand this the canonical id and the request. There is no
ensure-then-read pair to order correctly and no boolean to interpret.

The request is the demand document end to end — its **product set** is what
gets compiled. No compile profile is built from it on any path, and the source
is never copied into it.

This route can produce `runtimeClient`, `runtimeServer`, `ideCompanion`, and
`analysis`. The shared schema also exposes the bare tags `"publicApi"` and
`"declarations"`; `compileRequest()` refuses both for Vue and Svelte.

```ts
const host = await createHost();

host.upsert({ inputId: "/src/App.vue", source: sfcSource, fileKind: "vue" });

const result = host.compileRequest("/src/App.vue", {
  vue: {
    identity: { isProduction: false, forceJs: false },
    products: [
      { runtimeClient: { runtimeSourceMap: true } },
      {
        ideCompanion: {
          wantSourceMap: true,
          embedAmbientTypes: false,
          conditionalRootNarrowing: false,
          strictSlots: false,
          ideChunkBoundaries: false,
        },
      },
    ],
    options: { backend: "inferred", ssr: false, isCustomElement: [], babelParserPlugins: [] },
  },
});

// One row per requested product kind, in request order, tagged with the
// same `kind` spelling the request used.
for (const product of result.products) {
  if (product.kind === "runtimeClient") {
    // The assembled main module, the script, the compiled template, each
    // style block and each custom block — each with its own code and map.
    for (const node of product.nodes) console.log(node.node.kind, node.code.length);
  }
}
```

The request is discriminated by framework at the outermost level
(`{ vue: … }` / `{ svelte: … }`), and the arms are **mutually exclusive** — a
payload populating both is a TypeScript error as well as a decoder refusal.
Under TypeScript's default optional-property semantics, a known sibling tag
explicitly set to `undefined` is treated as absent. Every object is otherwise
closed: a key the schema does not declare — including the other framework's
option key — is refused by name.

The response carries `canonicalId` (after alias resolution), one deduplicated
`diagnostics` set for the whole compile, and the `products` rows. The
`analysis` row nests its payload under its own `analysis` key, so no field of
that payload can collide with the row's `kind` discriminant:

```ts
for (const product of result.products) {
  if (product.kind === "analysis") console.log(product.analysis.bindingOccurrences);
}
```

That payload is the **template** analysis snapshot — the value `getAnalysis()`
publishes under its `template` field — not the whole-file snapshot
`getAnalysis()` returns.

**Offset encodings and coordinate spaces differ by field.**
`diagnostics[].spanStart` / `spanEnd` and
`destructuredBlock.bindings[].sourceStart` / `sourceEnd` are **UTF-16 code
units into the registered source**, indexable against that JavaScript string.
`destructuredBlock.blockStart` / `blockEnd` are **UTF-16 code units into the
IDE product row's own `code`**, the generated IDE surface. The `analysis`
row's spans are **UTF-8 byte offsets into the registered source**, exactly as
`getAnalysis()` reports them — indexing a JS string with one is wrong on any
non-ASCII carrier.

**No compile cache slot.** Every call is a complete compile: this route
consults and publishes no cache slot, so two identical calls compile twice. The
profile-bearing `ensureIdeCompiled()` / `getIde()` pair _is_ cached, so a
per-keystroke editor loop that only needs the IDE surface stays cheaper there;
reach for `compileRequest()` when the demand is a fresh multi-product compile.

**Complete-only.** Every requested product the host can produce is present.
There is no partial response, no `null`, and no ensure boolean: a payload the
schema refuses, a request the compiler refuses, a framework arm the registered
carrier contradicts, an unproducible product kind, or an execution refusal all
**throw**, and no sibling product is published beside a refusal. A refusal
names the offending product the way the request spelled it (`publicApi`, not
`PublicApi`) and is thrown as the refusal-message string, not an `Error`
instance.

The profile-bearing `getIde()` / `ensureIdeCompiled()` / `getVirtualFile()`
methods are unchanged and keep serving every existing caller.

#### Shared module reference flow

`host.upsert()` now returns `moduleReferences`, which are classified as:

- `exact` — one literal specifier
- `finiteSet` — a bounded static candidate set
- `unknownDynamic` — intentionally unresolved

For browser-only consumers such as the playground, keep resolution in memory:

```ts
const update = host.upsert({
  inputId: "/src/App.vue",
  source: sfcSource,
});

const resolvedDeps = host.resolveKnownModuleReferenceDependencies(
  "/src/App.vue",
  update.moduleReferences,
  Object.keys(fileMap),
  [".ts", ".tsx", ".js", ".jsx", ".vue", "/index.ts"],
);

host.setImportDependencies("/src/App.vue", resolvedDeps);
```

This helper never reads from disk. It only considers the supplied `knownIds` and extension order, and it skips every `unknownDynamic` import. If you need a bundler to participate in resolution, use `collectResolvableModuleReferenceSpecifiers()` to hand only exact/finite candidates to that resolver, then call `setImportDependencies()` with the successfully resolved canonical IDs.

For IDE/provider consumers, importing `App.vue` resolves through the public `.vue.ts` surface. The internal IDE TSX/JSX virtual filename is not part of the public contract.

## Types

### Host Types

The `Host` class accepts and returns the same `Host*` types as `@verter/native`.

`@verter/wasm` declares the compile profile and the two request shapes that carry it, so
the browser binding owns its own compatibility contract for them. Their names and wire
shapes are identical to the `@verter/native` ones, and `src/index.test-d.ts` type-checks
that equivalence:

- `HostCompileProfile`
- `HostBlockOverrideRequest`
- `HostVirtualQuery`

### Typed compile request types

`compileRequest()`'s request is the shared framework-discriminated schema, tagged
differently for this binding: the arm name is the object's single key
(`{ vue: … }`, `{ analysis: … }`) where `@verter/native`'s decoder reads an
internal `framework` / `kind` field. Because the two wire forms are NOT
interchangeable, the browser forms carry a `Browser` prefix — a payload that
moved between the packages under a shared name would keep type-checking and be
refused by the other decoder at run time. Only the tagged wrappers are declared
here; every leaf option, identity and product shape is imported from
`@verter/native`'s generated projection, and `src/index.test-d.ts` pins the arm
sets, the arm payloads, and their mutual exclusivity to it:

- `BrowserHostCompileRequest`, `BrowserHostVueCompileRequest`,
  `BrowserHostSvelteCompileRequest`
- `BrowserHostRequestedProduct`
- `HostCompileIdentity`, `HostVueCompileOptions`, `HostSvelteCompileOptions`
- `HostRuntimeProductOptions`, `HostIdeProductOptions`, `HostAnalysisProductOptions`

The response types are unprefixed. Native now has a counterpart
(`HostCompileResponse` / `HostCompiledProduct` on `@verter/native`), but the
JavaScript envelopes are not the same object: native nests the IDE payload
under `ide`, stringifies `analysis`, and throws a structured `Error`; this
binding flattens the IDE DTO, returns `analysis` as an object, and throws a
string. The types here reuse the re-exported shared shapes wherever the route
serialises one (`HostDiagnosticsSnapshot`, `HostVirtualNodeKind`,
`HostVirtualMeta`, `HostIdeResponse`) rather than restating them:

- `HostCompileRequestResponse`
- `HostCompiledProduct`, `HostCompiledRuntimeProduct`, `HostCompiledIdeProduct`,
  `HostCompiledAnalysisProduct`
- `HostCompiledVirtualNode`, `HostDestructuredBlockMeta`,
  `HostTemplateAnalysisSnapshot`

The remaining types are re-exported from `@verter/native/host-types`:

- `HostConfig`
- `HostIdeResponse`
- `HostUpdateResult`
- `HostUpsertRequest`
- `HostVirtualFileResponse`
- `HostResolvedId`
- `HostRemoveResult`
- `HostVirtualNodeKind`

See the [@verter/native documentation](./native.md) for full definitions of the
re-exported types.

## Differences from @verter/native

| Feature                         | @verter/native                   | @verter/wasm                     |
| ------------------------------- | -------------------------------- | -------------------------------- |
| Environment                     | Node.js                          | Browser / Web Worker             |
| Binary format                   | Platform-specific `.node`        | WebAssembly `.wasm`              |
| `compile()`                     | Not available (use `VerterHost`) | Not available (use `VerterHost`) |
| `processStyle()`                | Available                        | Not available                    |
| `transformVueStyle()`           | Available                        | Not available                    |
| `prepareStyleForPreprocessor()` | Available                        | Not available                    |
| `analyzeStyle()`                | Available                        | Not available                    |
| `VerterHost`                    | Synchronous constructor          | Async via `createHost()`         |
| `getAnalysis()` return          | JSON `string`                    | Native JS `object`               |
| `compileRequest()`              | Available                        | Available                        |
| `compileRequest()` envelope     | Nested `ide`; `analysis` JSON string; structured `Error` | Flattened IDE DTO; `analysis` object; string throw |
| `compileRequests()`             | Available                        | Not available                    |
| `source` accepts                | `string \| Buffer`               | `string`                         |
