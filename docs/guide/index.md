# What is Verter?

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter is a Vue compiler, Language Server Protocol (LSP) implementation, and build tool. It converts Vue Single File Components (SFCs) into valid TSX for type checking and optimized render functions for production -- providing a stricter, more complete TypeScript experience for Vue than existing tools.

The project is a hybrid Rust + TypeScript monorepo. Rust owns SFC parsing, IDE TSX generation, runtime code generation, the shared semantic session, and the LSP server. TypeScript packages provide editor integration, TypeScript-provider adapters, protocol bindings, and bundler orchestration.

- [GitHub Repository](https://github.com/pikax/verter)
- [Online Playground](https://play.verterjs.dev)

## Why Verter?

Vue's Single File Component format combines `<template>`, `<script>`, and `<style>` blocks into one file. Getting full TypeScript support for this format requires translating Vue-specific syntax (directives, macros, scoped styles) into something TypeScript can understand.

Verter takes a different approach from existing tools:

- **Real TSX output** -- Verter generates actual valid TSX code that TypeScript can type-check directly, rather than relying on virtual file mappings. This means stricter type checking and fewer edge cases where types silently degrade to `any`.
- **Rust-powered compilation** -- Template compilation runs in Rust via NAPI-RS (for Node.js) and WASM (for browsers), delivering fast build times.
- **Unified build and IDE experience** -- The same compiler drives both your production builds (via the universal bundler plugin) and your IDE features (via the LSP server).

## Verter vs Volar

|                       | Verter                    | Volar                   |
| --------------------- | ------------------------- | ----------------------- |
| **Status**            | Pre-release (beta)        | Production-ready        |
| **Approach**          | SFC to real TSX           | Virtual file mapping    |
| **Compiler**          | Rust                      | TypeScript only         |
| **Type safety**       | Strict (valid TSX output) | General Vue development |
| **IDE**               | VS Code (Rust LSP binary) | VS Code + other editors |
| **Build integration** | Universal bundler plugin  | Separate (Vite plugin)  |

Volar is the established, production-ready solution for Vue IDE support. Verter is an experimental alternative exploring whether generating real TSX can provide a better TypeScript experience. If you need stability today, use Volar. If you want to experiment with stricter type checking and faster compilation, try Verter.

## Architecture

Verter consists of several packages that work together across two languages:

```mermaid
graph TB
    subgraph "IDE Layer"
        VSCode["verter-vscode<br/>(VS Code Extension)"]
    end
    subgraph "Rust Core"
        LSP["verter-lsp<br/>(Rust LSP binary, stdio)"]
        Session["verter_session<br/>(Host + Semantic Graph)"]
        Native["@verter/native<br/>(NAPI-RS Bindings)"]
        WASM["@verter/wasm<br/>(WASM Bindings)"]
        RustCore["verter_compiler<br/>(Runtime + IDE Codegen)"]
    end
    subgraph "Language Services"
        Provider["TSGO / tsserver<br/>(Type Provider)"]
        TSPlugin["@verter/typescript-plugin<br/>(TS Plugin)"]
        Shared["@verter/language-shared<br/>(Protocol Types)"]
    end
    subgraph "Build Tools"
        Unplugin["@verter/unplugin<br/>(Universal Bundler Plugin)"]
    end
    subgraph "Web"
        Playground["@verter/playground<br/>(Online Playground)"]
    end
    VSCode --> LSP
    VSCode --> Provider
    LSP --> Session
    Session --> RustCore
    LSP --> Provider
    LSP --> Shared
    Provider -->|loads for carrier imports| TSPlugin
    Native --> RustCore
    WASM --> RustCore
    Unplugin --> Native
    Playground --> WASM
```

### One Compiler Authority, Two Outputs

The Rust compiler parses each carrier and produces purpose-specific outputs from the same compiler authority:

```mermaid
flowchart LR
    SFC[".vue file"] --> RustCompiler["verter_compiler + verter_session<br/>(Rust)"]
    RustCompiler --> TSX["Typed TSX<br/>(for IDE analysis)"]
    RustCompiler --> Render["Render Functions<br/>(for runtime)"]
    TSX --> Provider["TSGO / tsserver"]
    Provider --> LSP["verter-lsp<br/>+ IDE Features"]
    Render --> Vite["Vite Build<br/>+ Production"]
```

**IDE output** -- `verter_compiler` generates valid TSX and source mappings. `verter_session` owns host-backed resolution, invalidation, and shared semantic caches; the LSP sends the generated TypeScript surface to TSGO or tsserver for editor features.

**Runtime output** -- The same Rust compiler emits optimized render functions for production builds. This runs through `@verter/unplugin` during Vite, webpack, Rollup, and other supported builds.

## Key Packages

| Package                     | Description                                                                             |
| --------------------------- | --------------------------------------------------------------------------------------- |
| `@verter/unplugin`          | Universal bundler plugin for Vite, Rollup, webpack, esbuild, rspack, Rolldown, and Farm |
| `@verter/native`            | Native Node.js bindings to the Rust compiler via NAPI-RS                                |
| `@verter/wasm`              | WASM bindings to the Rust compiler for browser use                                      |
| `@verter/types`             | TypeScript utility types for Vue component type inference                               |
| `@verter/typescript-plugin` | TypeScript language-service integration for Vue and the supported Svelte carrier surface |
| `@verter/language-shared`   | Shared protocol types between the VS Code extension and LSP server                      |
| `verter-vscode`             | VS Code extension that launches the Rust LSP binary                                     |

## Experimental Svelte compiler

Verter includes an experimental native Svelte client compiler tested against
the pinned `svelte@5.56.3` runtime. It is not presented as a general drop-in
replacement for the official compiler: supported client behavior is covered by
runtime and official-oracle tests, while unsupported runtime or server-output
surfaces fail closed with typed diagnostics. Published benchmark evidence is
limited to its explicit, equal-work fixture corpus and is not a general speed
or memory claim.

## Next Steps

- [Getting Started](./getting-started) -- Install Verter and set up your first project
- [Vite Integration](./vite) -- Full configuration guide for Vite
- [Other Bundlers](./other-bundlers) -- Rollup, esbuild, rspack, Rolldown, and Farm
