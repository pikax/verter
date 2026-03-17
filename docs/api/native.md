# @verter/native

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Native Node.js bindings via NAPI-RS. Provides high-performance SFC compilation for build tools and servers.

## Installation

```bash
pnpm add @verter/native
```

The correct platform-specific binary is automatically selected at runtime via optional dependencies. No additional configuration is needed.

## Platform Support

| Platform | Architecture | Package |
|----------|-------------|---------|
| Windows | x64 | `@verter/native-win32-x64-msvc` |
| Windows | ia32 | `@verter/native-win32-ia32-msvc` |
| Windows | ARM64 | `@verter/native-win32-arm64-msvc` |
| macOS | Universal | `@verter/native-darwin-universal` |
| macOS | x64 | `@verter/native-darwin-x64` |
| macOS | ARM64 | `@verter/native-darwin-arm64` |
| Linux | x64 (glibc) | `@verter/native-linux-x64-gnu` |
| Linux | x64 (musl) | `@verter/native-linux-x64-musl` |
| Linux | ARM64 (glibc) | `@verter/native-linux-arm64-gnu` |
| Linux | ARM64 (musl) | `@verter/native-linux-arm64-musl` |

## API

### `new Workspace(roots)`

Creates a workspace backed by the Rust VFS. The workspace is the **sole authority** for file access — no `fs` module is used in any JS package.

All file I/O methods are **async** (run on the libuv thread pool):

```ts
import { Workspace, VerterHost } from '@verter/native'

const ws = new Workspace(['/path/to/project'])

// File access
const content = await ws.readFile('/src/App.vue')       // string | null
const exists = await ws.fileExists('/src/App.vue')       // boolean
const entries = await ws.readDir('/src')                  // {path, isDir}[]
const files = await ws.walk('/src', ['node_modules'], ['.vue', '.ts']) // string[]

// File writes
await ws.writeFile('/src/new.ts', 'export const x = 1;')
await ws.deleteFile('/src/old.ts')

// Context-aware import resolution
const resolved = await ws.resolveImport('/src/App.vue', './Child.vue')
// phase: "codegen" | "provider"  —  kind: "esm" | "type" | "require" | "src"
const types = await ws.resolveImport('/src/App.vue', 'pkg', 'provider', 'type')

// Project configuration
ws.configureProjects([{
  root: '/project',
  workspaceRoot: '/project',
  compilerOptions: { baseUrl: '.', paths: { '@/*': ['src/*'] } },
}])
```

### `VerterHost.withWorkspace(config, workspace)`

Creates a `VerterHost` backed by a workspace. The workspace handles all file access and import resolution.

```ts
const ws = new Workspace(['/path/to/project'])
ws.configureProjects([...]) // configure first
const host = VerterHost.withWorkspace({ devMode: true }, ws)
```

### `processStyle(css, options)`

Process a CSS style block: apply scoping, CSS modules, and `v-bind()` replacement.

Called by the unplugin after preprocessing SCSS/Less/Stylus to valid CSS. For plain CSS blocks, the Rust compiler handles this inline during compilation.

```ts
import { processStyle } from '@verter/native'

const result = processStyle(css, {
  scopeId: 'a4f2eed6',
  scoped: true,
})
// result.code — transformed CSS
// result.moduleClasses — CSS module mappings
// result.vBindVars — replaced v-bind() expressions
```

**Parameters:**
- `css` (`string | Buffer`) -- Valid CSS as a string or Buffer (UTF-8 bytes)
- `options` (`ProcessStyleOptions`) -- Processing options

**Returns:** `ProcessStyleResult`

### `VerterHost`

In-memory virtual file host for multi-file compilation with caching and dependency tracking. This is the primary API for build tools that need to compile multiple `.vue` files with cross-file awareness.

```ts
import { VerterHost } from '@verter/native'

const host = new VerterHost({
  devMode: false,
  analysisLevel: 'full',
})
```

#### `host.resolve(rawId)`

Resolve a raw module ID (e.g., a virtual file request like `App.vue?vue&type=style&index=0&scoped&lang.css`) into a canonical ID and node kind.

```ts
const resolved = host.resolve('App.vue?vue&type=style&index=0')
// resolved.canonicalId — 'App.vue'
// resolved.nodeKind — { kind: 'style', index: 0 }
// resolved.bundlerId — full virtual file ID for the bundler
// resolved.lspId — LSP-compatible URI
```

**Returns:** `HostResolvedId | null`

#### `host.upsert(request)`

Register or update a file in the host. Handles parsing, caching, and change detection. Returns detailed information about what changed.

```ts
const result = host.upsert({
  inputId: '/path/to/App.vue',
  source: sfcSource, // string or Buffer
})
// result.canonicalId — resolved canonical ID
// result.changed — whether content actually changed
// result.sliceChanges — which blocks (script/template/style) changed
// result.externalSourceRequests — external src= attributes to resolve
// result.moduleReferences — import/require sites for dependency tracking
// result.diagnostics — parse-time diagnostics
```

**`HostUpsertRequest` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `inputId` | `string` | File path or identifier |
| `source` | `string \| Buffer` | SFC source code (Buffer avoids UTF-16 decode) |
| `canonicalId` | `string?` | Override canonical ID |
| `fileKind` | `string?` | `"vue"`, `"non_sfc"`, `"text"`, etc. |
| `aliases` | `string[]?` | Additional IDs that resolve to this file |

**Returns:** `HostUpdateResult`

`HostUpdateResult.moduleReferences` is the shared dependency-tracking surface for non-IDE consumers. Each reference reports:

- `analyzability: "exact"` when there is one literal specifier
- `analyzability: "finiteSet"` when static analysis found a bounded candidate set
- `analyzability: "unknownDynamic"` when the import is too dynamic to resolve safely

Only the exact and finite-set cases should feed dependency resolution. Unknown dynamic imports are intentionally left unresolved.

#### `host.getVirtualFile(query)`

Get a compiled virtual file from the host. Triggers compilation if needed.

```ts
const file = host.getVirtualFile({
  rawId: 'App.vue',
  compileProfile: {
    isProduction: true,
    ssr: false,
    hmrStrategy: 'none',
    sourceMap: true,
  },
})
// file.code — compiled output
// file.sourceMap — source map JSON string
// file.lang — output language ('ts', 'js', 'css', etc.)
// file.diagnostics — compilation diagnostics
```

**Returns:** `HostVirtualFileResponse`

#### `host.getPublicApi(canonicalId)`

Get the public TypeScript surface for a Vue SFC.

```ts
const publicApi = host.getPublicApi('/path/to/App.vue')
// publicApi.code — public `.vue.ts` module surface
// publicApi.sourceMap — source map JSON string
```

For provider and IDE consumers, importing `App.vue` resolves through the public `.vue.ts` surface. The internal IDE TSX/JSX virtual filename is not part of the public API contract.

**Returns:** `{ code: string; sourceMap?: string } | null`

#### `host.getIde(canonicalId, profile?)`

Get the IDE representation of a file for type checking. Used by the LSP and provider integration.

```ts
const ide = host.getIde('/path/to/App.vue', {
  target: 'ide',
})
// ide.code — valid TSX or JSX code
// ide.sourceMap — source map JSON string
// ide.isJsx — true for JSX, false for TSX
```

The IDE virtual filename used behind this API is internal. Consumers should rely on the returned code and source map, not on a specific virtual suffix such as `.vue.tsx`.

**Returns:** `HostIdeResponse | null`

#### `host.applyBlockOverrides(request)`

Apply preprocessed block overrides (template, script, style, or custom blocks). Used after an external preprocessor (Pug, SCSS, Less, Stylus, etc.) has transformed block content.

```ts
const result = host.applyBlockOverrides({
  canonicalId: '/path/to/App.vue',
  overrides: [
    { blockType: 'style', index: 0, code: processedCss },
  ],
})
```

**Returns:** `HostUpdateResult`

#### `host.listVirtualFiles(canonicalId)`

List all virtual file nodes for a given canonical ID.

```ts
const nodes = host.listVirtualFiles('/path/to/App.vue')
// [{ kind: 'main' }, { kind: 'script' }, { kind: 'style', index: 0 }]
```

**Returns:** `HostVirtualNodeKind[]`

#### `host.remove(canonicalOrAlias)`

Remove a file from the host and invalidate its cache.

```ts
const result = host.remove('/path/to/App.vue')
// result.canonicalId — the canonical ID that was removed
```

**Returns:** `HostRemoveResult | null`

#### `host.getAnalysis(canonicalOrAlias)`

Returns the static analysis snapshot for a file as a JSON string, or `null` if the file does not exist. When `analysisLevel` is not `"full"`, computes analysis on demand.

```ts
const analysisJson = host.getAnalysis('/path/to/App.vue')
if (analysisJson) {
  const analysis = JSON.parse(analysisJson)
}
```

**Returns:** `string | null`

#### `host.setImportDependencies(canonicalOrAlias, resolvedDeps)`

Sets the resolved import dependencies for a file, enabling Tier 2/3 smart invalidation (cross-file change tracking).

```ts
host.setImportDependencies('/path/to/App.vue', [
  '/path/to/types.ts',
  '/path/to/composables.ts',
])
```

Call this after you resolve `moduleReferences` from `upsert()`. The host does not guess unresolved dynamic imports for you.

#### `host.collectResolvableModuleReferenceSpecifiers(moduleReferences)`

Collect the exact and finite candidate specifiers from `HostUpdateResult.moduleReferences`, preserving encounter order and skipping `unknownDynamic` entries.

```ts
const { moduleReferences } = host.upsert({
  inputId: '/path/to/App.vue',
  source: sfcSource,
})

const candidates = host.collectResolvableModuleReferenceSpecifiers(moduleReferences)
// ['vue', './foo', './bar', './bar/index']
```

Use this when you want the bundler or another resolver to handle candidate lookup. This is the contract used by `@verter/unplugin`: delegate only exact/finite candidates, leave unknown dynamic imports unresolved, then pass the resolved canonical IDs back through `setImportDependencies()`.

**Returns:** `string[]`

#### `host.resolveKnownModuleReferenceDependencies(ownerCanonicalId, moduleReferences, knownIds, extensions?)`

Resolve exact and finite `moduleReferences` against an explicit in-memory file set, without reading from disk.

```ts
const knownIds = [
  '/src/App.vue',
  '/src/composables/useCount.ts',
  '/src/types.ts',
]

const resolvedDeps = host.resolveKnownModuleReferenceDependencies(
  '/src/App.vue',
  moduleReferences,
  knownIds,
  ['.ts', '.tsx', '.js', '.jsx', '.vue', '/index.ts'],
)

host.setImportDependencies('/src/App.vue', resolvedDeps)
```

This is the shared non-IDE resolver path used by the playground. Resolution is restricted to the provided `knownIds` plus the caller-supplied extension order. The helper remains disk-free and skips every `unknownDynamic` import.

**Parameters:**

- `ownerCanonicalId` (`string`) — canonical ID of the importing file
- `moduleReferences` (`HostModuleReference[]`) — `upsert()` output
- `knownIds` (`string[]`) — explicit file map or pre-known canonical IDs
- `extensions` (`string[]?`) — extension probe order for relative candidates

**Returns:** `string[]`

## Types

### `ProcessStyleOptions`

```ts
interface ProcessStyleOptions {
  /** Scope ID string (e.g., "a4f2eed6") */
  scopeId: string
  /** Whether this style block is scoped */
  scoped?: boolean
  /** Whether this is a CSS module block */
  isModule?: boolean
  /** Custom module name (default: "$style") */
  moduleName?: string
  /** Source filename for source map generation */
  filename?: string
  /** Whether to generate source maps */
  sourcemap?: boolean
}
```

### `ProcessStyleResult`

```ts
interface ProcessStyleResult {
  /** Transformed CSS code */
  code: string
  /** Source map as JSON string (if sourcemap was requested) */
  sourceMap?: string
  /** CSS module class mappings: [original, hashed][] */
  moduleClasses: [string, string][]
  /** Resolved CSS module name */
  moduleName?: string
  /** v-bind() expressions found and replaced */
  vBindVars: ProcessStyleVBind[]
}
```

### `HostConfig`

```ts
interface HostConfig {
  /** Enable development mode */
  devMode?: boolean
  /** Error handling policy */
  compileErrorPolicy?: 'strict' | 'strictError' | 'devServeLastKnownGood'
  /** LSP URI scheme */
  lspScheme?: string
  /** Maximum compile profiles cached per file */
  maxProfilesPerFile?: number
  /** File extensions to try during resolution */
  resolveExtensions?: string[]
  /** Static analysis level during upsert(). Default: "full" */
  analysisLevel?: 'full' | 'essential' | 'none'
}
```

### `HostCompileProfile`

Controls how a file is compiled. Different profiles produce different outputs (e.g., dev vs. prod, SSR vs. client).

```ts
interface HostCompileProfile {
  /** Override filename for the compilation */
  filename?: string
  /** Production mode optimizations */
  isProduction?: boolean
  /** Server-side rendering mode */
  ssr?: boolean
  /** HMR strategy: "none", "vite", or "webpack" */
  hmrStrategy?: 'none' | 'vite' | 'webpack'
  /** Component scope ID */
  componentId?: string
  /** Custom template delimiters */
  delimiters?: [string, string]
  /** Custom element tag names */
  customElements?: string[]
  /** Preserve HTML comments in output */
  comments?: boolean
  /** Custom Vue runtime module name */
  runtimeModuleName?: string
  /** Custom types module name */
  typesModuleName?: string
  /** Force Vapor mode output */
  forceVapor?: boolean
  /** Force JavaScript output (strip TypeScript) */
  forceJs?: boolean
  /** Generate source maps */
  sourceMap?: boolean
  /** Compilation target preset */
  target?: 'bundler' | 'ide' | 'analysis'
}
```

### `HostUpdateResult`

```ts
interface HostUpdateResult {
  canonicalId: string
  changed: boolean
  sliceChanges: HostSliceChanges
  changedVirtualNodes: HostVirtualNodeKind[]
  removedVirtualNodes: HostVirtualNodeKind[]
  changedVirtualIds: string[]
  removedVirtualIds: string[]
  changedLspIds: string[]
  removedLspIds: string[]
  diagnostics: HostDiagnosticsSnapshot
  externalSourceRequests: HostExternalSourceRequest[]
  importSpecifiers: HostScriptImportInfo[]
  moduleReferences: HostModuleReference[]
  preprocessorRequests: HostPreprocessorRequest[]
  parseDurationMs: number
}
```

### `HostVirtualFileResponse`

```ts
interface HostVirtualFileResponse {
  id: string
  code: string
  sourceMap?: string
  lang?: string
  stale: boolean
  diagnostics: HostDiagnosticsSnapshot
  meta: HostVirtualMeta
}
```

### `HostDiagnosticsSnapshot`

```ts
interface HostDiagnosticsSnapshot {
  diagnostics: HostDiagnostic[]
  hasErrors: boolean
}

interface HostDiagnostic {
  severity: 'error' | 'warning' | 'info'
  code: string
  message: string
  spanStart?: number
  spanEnd?: number
}
```

## Input Encoding

The native binding always receives bytes (`Buffer`) internally. The JS wrapper accepts both `string` and `Buffer` for convenience -- strings are automatically converted to UTF-8 Buffers before crossing the FFI boundary.

For best performance when reading files from disk, pass `Buffer` directly to avoid the intermediate UTF-16 string decode:

```ts
import { readFileSync } from 'fs'

// Optimal: pass Buffer directly (no string decode)
const source = readFileSync('App.vue')
host.upsert({ inputId: 'App.vue', source })

// Also works: string is converted to Buffer internally
const sourceStr = readFileSync('App.vue', 'utf-8')
host.upsert({ inputId: 'App.vue', source: sourceStr })
```
