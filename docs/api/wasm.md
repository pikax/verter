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

In-memory host facade exposed by the WASM runtime. Provides the same multi-file compilation API as `@verter/native`'s `VerterHost`, but running in the browser via WebAssembly.

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

The `Host` class exposes the same methods as `@verter/native`'s `VerterHost`:

| Method                                                                                      | Returns                    | Description                                                   |
| ------------------------------------------------------------------------------------------- | -------------------------- | ------------------------------------------------------------- |
| `resolve(rawId)`                                                                            | `HostResolvedId \| null`   | Resolve raw ID to canonical ID                                |
| `upsert(request)`                                                                           | `HostUpdateResult`         | Register/update a file                                        |
| `applyBlockOverrides(request)`                                                              | `HostUpdateResult`         | Apply preprocessed block overrides                            |
| `getIde(canonicalId, profile?)`                                                             | `HostIdeResponse \| null`  | Get TSX or JSX for type checking                              |
| `getVirtualFile(query)`                                                                     | `HostVirtualFileResponse \| null` | Get compiled virtual file (`null` when the node does not exist) |
| `listVirtualFiles(canonicalId)`                                                             | `HostVirtualNodeKind[]`    | List virtual nodes for a file                                 |
| `remove(canonicalOrAlias)`                                                                  | `HostRemoveResult \| null` | Remove file from host                                         |
| `getAnalysis(canonicalOrAlias)`                                                             | `unknown \| null`          | Get analysis snapshot (native JS object)                      |
| `setImportDependencies(id, deps)`                                                           | `void`                     | Set resolved import dependencies                              |
| `collectResolvableModuleReferenceSpecifiers(moduleReferences)`                              | `string[]`                 | Return exact/finite candidate specifiers in encounter order   |
| `resolveKnownModuleReferenceDependencies(ownerId, moduleReferences, knownIds, extensions?)` | `string[]`                 | Resolve exact/finite candidates against an in-memory file set |

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
  overrides: [{
    correlationToken: pending.correlationToken,
    blockToken: pending.blockToken,
    ownerRevision: pending.ownerRevision,
    artifactToken: pending.artifactToken,
    basisToken: pending.basisToken,
    sourceSpaceToken: pending.sourceSpaceToken,
    code,
    codeHash: await hashBlockContent(code),
  }],
});
```

The host validates the current revision, artifact, basis, source space, code
hash, and optional source-map hash after the asynchronous processor completes.
It refuses stale or mismatched results without mutating its caches.

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
| `source` accepts                | `string \| Buffer`               | `string`                         |
