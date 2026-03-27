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

Load the WASM module. Must be called before `compileSync()` or constructing a `Host`. Safe to call multiple times -- only initializes once. Subsequent calls return immediately.

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

### `compile(input, options?)`

Compile a Vue SFC to JavaScript. Auto-initializes the WASM module if needed.

```ts
import { compile } from "@verter/wasm";

const result = await compile(source, { filename: "App.vue" });
// result.code — compiled JavaScript
// result.sourceMap — source map JSON string
// result.codeWithSourceMap — code with inline source map appended
// result.styles — compiled CSS blocks
// result.errors — compilation diagnostics
// result.durationMs — Rust pipeline timing
```

**Parameters:**

- `input` (`string | Uint8Array`) -- Vue SFC source code
- `options` (`CodegenOptions?`) -- Optional compilation options

**Returns:** `Promise<CodegenResult>`

### `compileSync(input, options?)`

Synchronous compilation. Requires `initialize()` to have been called first. Throws if the WASM module has not been initialized.

```ts
import { initialize, compileSync } from "@verter/wasm";

await initialize();

const result = compileSync(source, { filename: "App.vue" });
```

**Parameters:**

- `input` (`string | Uint8Array`) -- Vue SFC source code
- `options` (`CodegenOptions?`) -- Optional compilation options

**Returns:** `CodegenResult`

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
| `getVirtualFile(query)`                                                                     | `HostVirtualFileResponse`  | Get compiled virtual file                                     |
| `listVirtualFiles(canonicalId)`                                                             | `HostVirtualNodeKind[]`    | List virtual nodes for a file                                 |
| `remove(canonicalOrAlias)`                                                                  | `HostRemoveResult \| null` | Remove file from host                                         |
| `getAnalysis(canonicalOrAlias)`                                                             | `unknown \| null`          | Get analysis snapshot (native JS object)                      |
| `setImportDependencies(id, deps)`                                                           | `void`                     | Set resolved import dependencies                              |
| `collectResolvableModuleReferenceSpecifiers(moduleReferences)`                              | `string[]`                 | Return exact/finite candidate specifiers in encounter order   |
| `resolveKnownModuleReferenceDependencies(ownerId, moduleReferences, knownIds, extensions?)` | `string[]`                 | Resolve exact/finite candidates against an in-memory file set |

See the [@verter/native documentation](./native.md) for detailed descriptions of each method and their parameter types.

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

### `CodegenOptions`

```ts
interface CodegenOptions {
  /** Filename for source map generation */
  filename?: string;
  /** Production mode — affects component ID and optimizations */
  isProduction?: boolean;
  /** Custom component ID (overrides auto-generation) */
  componentId?: string;
  /** Generate TSX output alongside JavaScript. Default: false */
  includeTsx?: boolean;
}
```

### `CodegenResult`

```ts
interface CodegenResult {
  /** The compiled JavaScript code */
  code: string;
  /** Source map as JSON string */
  sourceMap: string;
  /** Code with inline source map appended */
  codeWithSourceMap: string;
  /** Compiled CSS blocks from <style> tags */
  styles: CompiledStyleBlock[];
  /** Scope ID for scoped styles (e.g., "data-v-a4f2eed6"). Empty if none */
  scopeId: string;
  /** Compilation diagnostics (errors, warnings) */
  errors: WasmDiagnostic[];
  /** Time taken for the Rust pipeline in milliseconds */
  durationMs: number;
  /** Generated TSX code (when includeTsx is true) */
  tsx: string;
  /** Compiled CSS (scoped selectors applied, v-bind replaced) */
  css: string;
  /** Time taken for TSX generation in milliseconds */
  tsxDurationMs: number;
}
```

### `CompiledStyleBlock`

```ts
interface CompiledStyleBlock {
  /** Compiled CSS code */
  code: string;
  /** Whether this style block is scoped */
  scoped: boolean;
  /** Style language (css, scss, less, stylus) */
  lang: string | null;
  /** Whether this is a CSS module block */
  isModule: boolean;
  /** CSS module class mappings: [original, hashed][] */
  moduleClasses: [string, string][];
  /** CSS processing errors */
  errors: string[];
}
```

### `WasmDiagnostic`

```ts
interface WasmDiagnostic {
  /** Severity level: "error", "warning", or "info" */
  severity: string;
  /** Vue-compatible error code (e.g., "XMissingEndTag") */
  code: string;
  /** Human-readable error message */
  message: string;
  /** Source span start (byte offset) */
  spanStart?: number;
  /** Source span end (byte offset) */
  spanEnd?: number;
}
```

### Host Types

The `Host` class accepts and returns the same `Host*` types as `@verter/native`. These types are re-exported from `@verter/native/host-types`:

- `HostConfig`
- `HostCompileProfile`
- `HostIdeResponse`
- `HostUpdateResult`
- `HostUpsertRequest`
- `HostVirtualFileResponse`
- `HostVirtualQuery`
- `HostResolvedId`
- `HostRemoveResult`
- `HostVirtualNodeKind`

See the [@verter/native documentation](./native.md) for full type definitions.

## Input Encoding

The WASM `compile()` and `compileSync()` functions accept both `string` and `Uint8Array` input. When `Uint8Array` is provided and the WASM binary supports `compileBytes`, the bytes are passed directly without UTF-8 decode. Otherwise, the `Uint8Array` is decoded via `TextDecoder` before compilation.

```ts
// String input
const result = await compile("<template><div>Hello</div></template>");

// Uint8Array input (e.g., from fetch)
const response = await fetch("/App.vue");
const bytes = new Uint8Array(await response.arrayBuffer());
const result = await compile(bytes, { filename: "App.vue" });
```

## Differences from @verter/native

| Feature                | @verter/native                   | @verter/wasm                     |
| ---------------------- | -------------------------------- | -------------------------------- |
| Environment            | Node.js                          | Browser / Web Worker             |
| Binary format          | Platform-specific `.node`        | WebAssembly `.wasm`              |
| `compile()`            | Not available (use `VerterHost`) | Available (standalone)           |
| `compileSync()`        | Not available                    | Available (after `initialize()`) |
| `processStyle()`       | Available                        | Not available (use `compile()`)  |
| `VerterHost`           | Synchronous constructor          | Async via `createHost()`         |
| `getAnalysis()` return | JSON `string`                    | Native JS `object`               |
| `source` accepts       | `string \| Buffer`               | `string`                         |
