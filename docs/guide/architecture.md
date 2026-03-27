# Architecture

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter is a hybrid Rust + TypeScript monorepo. Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server, while TypeScript packages handle the SFC-to-TSX transformation, IDE integration, and bundler plugins.

## System Overview

```mermaid
graph TB
    subgraph "IDE Layer"
        VSCode["verter-vscode<br/>(VS Code Extension)"]
    end
    subgraph "Rust"
        LSP["verter-lsp<br/>(Rust LSP binary, stdio)"]
        Native["@verter/native<br/>(NAPI-RS Bindings)"]
        WASM["@verter/wasm<br/>(WASM Bindings)"]
        VFS["verter_vfs<br/>(Virtual Filesystem)"]
        RustCore["verter_core<br/>(Rust Template Compiler)"]
    end
    subgraph "Language Services"
        TSPlugin["@verter/typescript-plugin<br/>(TS Plugin)"]
        Shared["@verter/language-shared<br/>(Protocol Types)"]
    end
    subgraph "Transformation"
        Core["@verter/core<br/>(SFC → TSX)"]
        Types["@verter/types<br/>(Type Utilities)"]
    end
    subgraph "Build Tools"
        Unplugin["@verter/unplugin<br/>(Universal Bundler Plugin)"]
    end
    subgraph "Metadata"
        ComponentMeta["@verter/component-meta<br/>(Metadata Extraction + Type IR)"]
    end
    subgraph "Web"
        Playground["@verter/playground<br/>(Online Playground)"]
    end
    VSCode --> LSP
    VSCode --> TSPlugin
    LSP --> RustCore
    LSP --> Shared
    TSPlugin --> Core
    Core --> Types
    Native --> VFS
    Native --> RustCore
    WASM --> RustCore
    Unplugin --> Native
    Playground --> WASM
    ComponentMeta --> Native
    ComponentMeta -.-> WASM
```

## Dual Compilation Pipeline

Verter has two distinct compilation paths, each optimized for its purpose:

```mermaid
flowchart LR
    SFC[".vue file"] --> TSCore["@verter/core<br/>(TypeScript)"]
    SFC --> RustCompiler["verter_core<br/>(Rust)"]
    TSCore --> TSX["Typed TSX<br/>(for IDE analysis)"]
    RustCompiler --> Render["Render Functions<br/>(for runtime)"]
    TSX --> LSP["verter-lsp<br/>+ IDE Features"]
    Render --> Vite["Vite Build<br/>+ Production"]
```

**TypeScript pipeline** (`@verter/core`) -- Transforms `.vue` files into valid TSX using MagicString for sourcemap preservation. This output is consumed by the LSP server and TypeScript plugin to provide IDE features like hover types, completions, go-to-definition, and diagnostics.

**Rust pipeline** (`verter_core`) -- Compiles Vue templates into optimized render functions (VDOM or Vapor mode) for production builds. This runs through `@verter/unplugin` during your Vite/webpack/Rollup build, and also powers the LSP's template analysis.

Both pipelines share the same Vue SFC input and produce consistent results -- the TypeScript path prioritizes type accuracy while the Rust path prioritizes runtime performance.

## Repository Structure

| Directory                    | Purpose                                                                  |
| ---------------------------- | ------------------------------------------------------------------------ |
| `crates/verter_core/`        | Rust template compiler                                                   |
| `crates/verter_vfs/`         | Virtual filesystem: sole authority for file access and import resolution |
| `crates/verter_analysis/`    | Static analysis: imports, exports, bindings, types                       |
| `crates/verter_host/`        | In-memory file host: caching, dependencies, multi-file compilation       |
| `crates/verter_diagnostics/` | Diagnostic engine: 22+ lint rules, visitor, DiagnosticSet                |
| `crates/verter_actions/`     | Code actions engine: quick fixes, refactoring                            |
| `crates/verter_lsp/`         | Rust LSP server binary (stdio)                                           |
| `crates/verter_ffi/`         | FFI types for NAPI/WASM boundaries                                       |
| `crates/verter_napi/`        | Native Node.js bindings (NAPI-RS)                                        |
| `crates/verter_wasm/`        | WASM bindings (wasm-bindgen)                                             |
| `packages/core/`             | `@verter/core` -- SFC parser & TSX transformer                           |
| `packages/types/`            | `@verter/types` -- TypeScript utility types                              |
| `packages/native/`           | `@verter/native` -- Native binding loader                                |
| `packages/wasm/`             | `@verter/wasm` -- WASM binding wrapper                                   |
| `packages/unplugin/`         | `@verter/unplugin` -- Universal bundler plugin                           |
| `packages/component-meta/`   | `@verter/component-meta` -- Component metadata extraction + Type IR      |
| `packages/vue-vscode/`       | VS Code extension                                                        |

## Async File Scheduler

The `verter_scheduler` crate provides per-file async staging with a priority queue. Files progress independently through **Source → Analysis → Artifact** stages. Cross-file blocking (macro type deps, external `src` attributes) is declarative — the scheduler manages wakeups via its `BlockerRegistry`.

Key concepts:

- **FileNode**: per-file state with ArcSwap snapshots and an atomic generation counter
- **Priority tiers**: Critical (hover/completion) > Interactive (did_open) > Background (workspace scan) > Maintenance
- **Generation fencing**: stale snapshots are invisible — `current_analysis()` returns `None` if the generation doesn't match
- **StageExecutor trait**: the host plugs in real parse/compile logic; the scheduler provides coordination

The scheduler is integrated into `VerterHost` via the `scheduler` feature flag. During `upsert()`, the host populates both its legacy `files` map and the scheduler's FileNode snapshots in parallel.

## Rust Compilation Pipeline

The Rust compiler uses an AST-based pipeline with five phases. The `compile()` function in `verter_core` orchestrates the entire flow:

```mermaid
flowchart TD
    Source["Vue SFC Source"] --> Tokenizer["Tokenizer<br/>(byte-level SFC tokenization)"]
    Tokenizer --> Parser["Parser<br/>(arena-based template AST + blocks)"]
    Parser --> Style["Style<br/>(v-bind scan + CSS processing)"]
    Style --> Script["Script<br/>(macro expansion, bindings, wrapper)"]
    Script --> Template["Template<br/>(render function codegen)"]
    Template --> Output["Compiled Output<br/>(JS + source map)"]
```

### Phase 1: Tokenizer

A zero-copy byte-level tokenizer scans the raw SFC source. It identifies `<template>`, `<script>`, and `<style>` block boundaries, tag names, attributes, and text content. The tokenizer operates directly on bytes without allocating intermediate string copies.

### Phase 2: Parser

The parser consumes tokenizer output and builds an arena-based template AST. Elements, attributes, directives, text nodes, and interpolations are allocated in a flat arena with O(1) parent/child navigation. Structural directives (`v-if`, `v-for`, `v-slot`, `v-once`, `ref`) are extracted from props and cached as dedicated fields on element nodes for efficient access during codegen.

Script and style blocks are extracted as separate root nodes with their content spans and attributes (lang, scoped, module, src).

### Phase 3: Style

The style phase scans `<style>` blocks for `v-bind()` expressions (CSS values bound to reactive data) and processes CSS features:

- **Scoped CSS** -- Inserts `[data-v-xxx]` attribute selectors for style isolation
- **CSS Modules** -- Hashes class names for local scoping
- **`v-bind()` in CSS** -- Extracts expressions for runtime CSS variable injection
- **CSS Variable Analysis** -- Extracts custom property definitions (`--name: value`), `var()` references with fallbacks, and tracks `v-bind()` → generated variable name mappings for cross-component CSS variable flow analysis

### Phase 4: Script

The script phase processes `<script setup>` content:

- **Macro expansion** -- Transforms `defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `defineOptions`, and `withDefaults` into their runtime equivalents
- **Binding extraction** -- Identifies all declared variables, imports, and their binding types (setup, data, props, etc.) for template codegen
- **Component wrapper** -- Generates the component definition that wires props, emits, setup function, and render function together
- **Companion script merging** -- If both `<script>` and `<script setup>` exist, merges the companion script's exports into the setup component

### Phase 5: Template

The final phase walks the template AST and generates render function code. Two backends are available:

- **VDOM** -- Generates `_createElementVNode()`, `_createVNode()`, `_createTextVNode()` calls with patch flags for Vue's virtual DOM runtime
- **Vapor** -- Generates `_template()`, `_renderEffect()`, `_setText()` calls for Vue's upcoming Vapor mode (no virtual DOM)

Both backends share a common DFS tree walker and binding resolver that determines whether each template expression references a setup binding, prop, data property, or global.

### Output

The pipeline produces a `CodeTransform` -- a chunk-based deferred mutation engine (similar to MagicString) that tracks original source positions. From this, Verter emits:

- **JavaScript output** -- The compiled component module
- **Source map** -- VLQ-encoded source map mapping compiled output back to the original `.vue` file

## TSX Codegen Path

In addition to the VDOM/Vapor render function backends, the Rust compiler has a separate TSX codegen path (`crates/verter_core/src/tsx/`). This path converts Vue templates into valid JSX that TypeScript can type-check:

- `v-if`/`v-else-if`/`v-else` become ternary expressions
- `v-for` becomes `.map()` calls
- `:prop` bindings become `prop={expression}` JSX attributes
- `@event` handlers become `onEvent={handler}` JSX attributes
- `v-model` becomes the appropriate prop + event pair

The LSP server uses this TSX output for type-checking via TSGO (TypeScript's Go-based type checker), enabling hover types, diagnostics, and completions that reflect the actual template structure.

## Virtual Filesystem (VFS)

All workspace file access and import resolution flows through `verter_vfs`. This crate is the **single authority** — no code outside `NativeFs` touches `std::fs`, and the host never does its own heuristic resolution.

```mermaid
graph TB
    subgraph "Consumers"
        Host["verter_host<br/>(Compilation Host)"]
        LSP["verter_lsp<br/>(Language Server)"]
        Unplugin["@verter/unplugin<br/>(Bundler Plugin)"]
        Meta["@verter/component-meta<br/>(Metadata Extraction)"]
    end

    subgraph "verter_vfs"
        WS["WorkspaceAccess Trait"]
        Engine["Engine<br/>(overlay → snapshot → resolver)"]
        Resolver["ProjectResolver<br/>(tsconfig, aliases, node_modules)"]
        Edges["EdgeStore<br/>(forward/reverse deps)"]
        NativeFs["NativeFs<br/>(sole std::fs boundary)"]
    end

    Host -->|"resolve_import(ctx)"| WS
    LSP --> WS
    Unplugin -->|"async readFile/fileExists/walk"| WS
    Meta -->|"async readFile/readDir/walk"| WS
    WS --> Engine
    Engine --> Resolver
    Engine --> Edges
    Engine --> NativeFs
```

### Context-Aware Resolution

Import resolution is context-sensitive. The same specifier can resolve to different files depending on the resolution context:

| Context                        | Export Conditions                | Legacy Fields                  |
| ------------------------------ | -------------------------------- | ------------------------------ |
| `(CodegenBlocker, EsmImport)`  | `["import", "default"]`          | `["module", "main"]`           |
| `(CodegenBlocker, TypeImport)` | `["types", "import", "default"]` | `["types", "typings", "main"]` |
| `(ProviderGraph, *)`           | `["types", "import", "default"]` | `["types", "typings", "main"]` |
| `(*, RequireCall)`             | `["require", "default"]`         | `["main"]`                     |

For example, `import { Foo } from 'pkg'` during codegen resolves to `pkg/index.js` (runtime entry), while `import type { Foo } from 'pkg'` resolves to `pkg/index.d.ts` (type entry).

### Workspace API (Node.js)

JS consumers access the filesystem exclusively through the `Workspace` class from `@verter/native`. All methods are **async** (Promise-based, runs on libuv thread pool):

```ts
import { Workspace, VerterHost } from "@verter/native";

const ws = new Workspace(["/path/to/project"]);

// File access — all async
const content = await ws.readFile("/path/to/file.vue");
const exists = await ws.fileExists("/path/to/file.ts");
const entries = await ws.readDir("/path/to/dir");
const files = await ws.walk("/path", ["node_modules"], [".vue", ".ts"]);

// Resolution
const resolved = await ws.resolveImport("/src/App.vue", "./Child.vue");

// Project configuration
ws.configureProjects([
  {
    root: "/path/to/project",
    workspaceRoot: "/path/to/project",
    compilerOptions: { baseUrl: ".", paths: { "@/*": ["src/*"] } },
  },
]);

// Create host backed by workspace
const host = VerterHost.withWorkspace({}, ws);
```

No `node:fs` imports are used in any JS package. The `Workspace` object is the sole filesystem authority from JavaScript.

### File Read Priority

```
1. Overlay   (active editor buffer — set via notifyUpsert)
2. Snapshot  (cached content — populated on first read)
3. Disk      (NativeFs — FilesystemWorkspace only)
```

### Resolution Priority

```
1. Exact resolutions  (authoritative — injected by bundler/LSP via setImportDependencies)
2. Project resolver   (tsconfig paths, workspace aliases, node_modules package.json)
3. None               (no heuristic fallback, no extension guessing)
```

## Next Steps

- [Features](./features) -- Type safety features overview
- [Performance](./performance) -- Compilation benchmarks
- [Diagnostics & Linting](./linting) -- Built-in diagnostic rules
- [Cross-File Optimization](./cross-file-optimization) -- Whole-program analysis
