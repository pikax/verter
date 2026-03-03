# Verter

A Vue compiler, Language Server Protocol (LSP) implementation, and build tool — built as a hybrid Rust + TypeScript monorepo.

> [!WARNING]
> **This project is experimental and under active development.** APIs, architecture, and package boundaries may change without notice. Use at your own risk and please report any issues you find.

> [!IMPORTANT]
> The generated TSX is syntactically valid TypeScript/TSX used for **type analysis only** — it's not meant to be executed or compiled as actual JSX/TSX code.

## Project Vision

Verter started as a Vue LSP and SFC-to-TSX transformation tool for VS Code, aiming to provide better TypeScript support than [Volar](https://github.com/vuejs/language-tools). The project is now evolving into a **full Vue compiler**:

- **Today**: TypeScript handles SFC-to-TSX transformation (for IDE type analysis), while Rust handles template compilation to optimized render functions (for runtime).
- **Future**: The Rust compiler (`verter_core`) will progressively take over responsibilities currently handled by the TypeScript packages, becoming the single compilation engine for both IDE analysis and runtime output.

## Features

- **Full TypeScript Support**: Converts `.vue` files to typed TSX representations, enabling complete TypeScript type inference
- **Vue 3 Support**: Optimized for Vue 3 with Composition API and Script Setup
- **Options API Support**: While Script Setup receives more attention, Options API is fully supported
- **Strict Type Safety**: Built with a "strict first" approach to type safety
- **JSX/TSX Interoperability**: SFCs can be seamlessly used in JSX/TSX projects
- **Generic Component Handling**: Full support for generic Vue components with proper constructor typing
- **Automatic Event Handler Type Inference**: Infers parameter types for functions used as template event handlers
- **Fully Typed Vue Directives**: Complete type safety for directives with strict modifier, argument, and value validation
- **Rust-Powered Template Compilation**: High-performance template-to-render-function compilation via native bindings or WASM
- **Built-in Linting**: ~164 lint rules across 11 categories (Vue, a11y, CSS, performance, security, and more) — runs natively in Rust, no ESLint needed
- **MCP Server**: Built-in Model Context Protocol server with 36+ Vue analysis tools for AI agents
- **TypeScript Type Provider**: Delegates type checking to TSGO (fast Go binary) or tsserver (workspace TS version)

### Generic Components

Verter provides improved handling for generic Vue components, respecting Vue constructors with proper type inference:

```vue
<!-- Comp.vue -->
<script setup lang="ts" generic="T extends string">
defineProps<{
  name: T;
}>();

defineSlots<
  Record<T & string, (args: { test: T }) => any> & {
    header: (a: { foo: string }) => any;
  }
>();
</script>
```

```typescript
import Comp from "./Comp.vue";

const foo = {} as InstanceType<typeof Comp<"myName">>;
foo.$props.name; // Type: 'myName'
```

### Automatic Event Handler Type Inference

Verter automatically infers types for function parameters used as event handlers in templates:

```vue
<script setup lang="ts">
// No type annotation needed - Verter infers the type automatically
function handleClick(e) {
  // e is inferred as MouseEvent from HTMLElementEventMap["click"]
  console.log(e.clientX, e.clientY);
}
</script>

<template>
  <button @click="handleClick">Click me</button>
</template>
```

This works for native HTML elements (via `HTMLElementEventMap`), Vue components (via emits/props definitions), and multi-parameter event handlers.

### Fully Typed Vue Directives

```vue
<script setup lang="ts">
const vColor: Directive<HTMLElement, string, "red" | "blue"> = (el, binding) => {
  el.style.color = binding.value;
};
</script>

<template>
  <span v-color.blue="'red'" />
  <!-- valid -->
  <span v-color.green="'red'" />
  <!-- type error: invalid modifier -->
  <input v-model.number.trim="count" />
</template>
```

## Performance

Verter's Rust-powered compiler is significantly faster than Vue's JavaScript-based compiler. Benchmarks compare template compilation of real-world Vue SFCs using Verter's native NAPI-RS bindings against `@vue/compiler-sfc`. On average, Verter compiles templates **~9x faster** than Vue.

| Fixture | Size | Vue (ops/s) | Verter (ops/s) | Speedup | Throughput |
|---|---|---:|---:|---:|---:|
| tiny-template | 42 B | 21,240 | 112,368 | **5.3x** | 4.50 MB/s |
| simple-interactive | 242 B | 3,140 | 52,440 | **16.7x** | 12.10 MB/s |
| list-rendering | 1.3 KB | 1,141 | 15,066 | **13.2x** | 18.85 MB/s |
| conditional-heavy | 2.0 KB | 1,288 | 14,928 | **11.6x** | 29.27 MB/s |
| form-component | 4.3 KB | 750 | 7,359 | **9.8x** | 30.58 MB/s |
| composition-heavy | 9.0 KB | 368 | 3,644 | **9.9x** | 31.86 MB/s |
| template-heavy | 8.5 KB | 1,077 | 2,914 | **2.7x** | 24.20 MB/s |
| kitchen-sink | 26.7 KB | 141 | 1,106 | **7.8x** | 28.86 MB/s |
| **20k files (stress test)** | 127 MB | 30.8s | 5.0s | **6.1x** | 25.16 MB/s |

> **Average speedup: 9.2x** across all fixtures. Throughput scales from ~4.5 MB/s on tiny files to ~32 MB/s on larger components, with memory usage significantly lower than Vue's compiler.

These benchmarks run in CI on every PR (triggered via `/benchmark` comment) and use 8 Vue SFC fixtures ranging from trivial templates to a 27 KB kitchen-sink component, plus a 20,000-file stress test. See the [benchmark package](./packages/benchmark/) for fixture sources and methodology.

## Why Verter?

Since the Vetur days, Vue has struggled with type safety and tooling quality. Vue 3 and Volar brought significant improvements, but challenges remain. Verter aims to provide the **best possible TypeScript experience for Vue** by converting SFCs into typed TSX representations that TypeScript's language service can analyze directly.

### Verter vs Volar

| Aspect   | Verter                             | Volar                    |
| -------- | ---------------------------------- | ------------------------ |
| Maturity | Experimental / Alpha               | Production-ready         |
| Approach | SFC → Typed TSX representation     | Virtual file mapping     |
| Compiler | Rust (template) + TypeScript (SFC) | TypeScript only          |
| Focus    | Best TypeScript integration        | Feature-rich IDE support |
| Use case | When you need strict type safety   | General Vue development  |

> [!NOTE]
> If you haven't encountered specific issues with Volar, there's no reason to switch. Verter is for developers who need enhanced TypeScript support and are comfortable with experimental software.

### Type Provider

The LSP delegates TypeScript type checking to an external process. Two backends are supported:

| Backend | Protocol | Use Case |
|---------|----------|----------|
| **TSGO** (Go binary) | LSP over stdio | Fast, native TS checking (preview) |
| **tsserver** (Node.js) | Newline-delimited JSON | Workspace TS version, plugin support |

Set `verter.typeProvider` in VS Code settings to `auto` (default), `tsgo`, `tsserver`, or `off`.

> [!WARNING]
> **TSGO limitation**: Re-exported `.vue` components (e.g., barrel files like `export { default as MyComp } from './MyComp.vue'`) may lose their typing when imported in another SFC. This is why `auto` mode defaults to tsserver when a workspace TypeScript installation is found. If you experience missing types with TSGO, switch to `tsserver`.

## Architecture

Verter is a hybrid Rust + TypeScript monorepo. Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter-lsp` binary, communicates over stdio), while TypeScript packages handle the SFC-to-TSX transformation and IDE integration.

### System Overview

```mermaid
graph TB
    subgraph "IDE Layer"
        VSCode["verter-vscode<br/>(VS Code Extension)"]
    end

    subgraph "Rust"
        LSP["verter-lsp<br/>(Rust LSP binary, stdio)"]
        Native["@verter/native<br/>(NAPI-RS Bindings)"]
        WASM["@verter/wasm<br/>(WASM Bindings)"]
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

    subgraph "Web"
        Playground["@verter/playground<br/>(Online Playground)"]
    end

    VSCode --> LSP
    VSCode --> TSPlugin
    LSP --> RustCore
    LSP --> Shared
    TSPlugin --> Core
    Core --> Types
    Native --> RustCore
    WASM --> RustCore
    Unplugin --> Native
    Playground --> WASM
```

### Dual Compilation Pipeline

```mermaid
flowchart LR
    SFC[".vue file"] --> TSCore["@verter/core<br/>(TypeScript)"]
    SFC --> RustCompiler["verter_core<br/>(Rust)"]
    TSCore --> TSX["Typed TSX<br/>(for IDE analysis)"]
    RustCompiler --> Render["Render Functions<br/>(for runtime)"]
    TSX --> LSP["verter-lsp<br/>+ IDE Features"]
    Render --> Vite["Vite Build<br/>+ Production"]
```

### Repository Structure

```
verter/
├── crates/                        # Rust crates
│   ├── verter_core/               # Core template compiler (pure Rust)
│   ├── verter_analysis/           # Static analysis: imports, exports, bindings, type resolution
│   ├── verter_host/               # In-memory file host: caching, dependency tracking
│   ├── verter_diagnostics/        # Diagnostic engine: ~164 lint rules, visitor, DiagnosticSet
│   ├── verter_actions/            # Code actions: quick fixes, refactoring
│   ├── verter_lsp/                # Rust LSP server binary (stdio)
│   ├── verter_mcp/                # MCP server binary (stdio + HTTP)
│   ├── verter_ffi/                # FFI types: shared serializable structs for NAPI/WASM
│   ├── verter_span/               # Typed span types (Span, RelativeSpan, GeneratedSpan)
│   ├── verter_bench/              # Benchmarks and comparison examples
│   ├── verter_napi/               # Native Node.js bindings (NAPI-RS)
│   └── verter_wasm/               # WASM bindings (wasm-bindgen)
├── packages/                      # TypeScript packages
│   ├── core/                      # @verter/core — SFC → TSX transformation
│   ├── types/                     # @verter/types — TypeScript utility types
│   ├── native/                    # @verter/native — Native binding loader
│   ├── wasm/                      # @verter/wasm — WASM binding wrapper
│   ├── unplugin/                  # @verter/unplugin — Universal bundler plugin
│   ├── language-shared/           # @verter/language-shared — Shared protocol types
│   ├── typescript-plugin/         # @verter/typescript-plugin — TS plugin
│   ├── oxc-bindings/              # @verter/oxc-bindings — OXC parser helper
│   ├── playground/                # @verter/playground — Online playground
│   ├── vue-vscode/                # verter-vscode — VS Code extension
│   └── example/                   # Example project
├── docs/                          # Documentation (VitePress site)
└── scripts/                       # Build and utility scripts
```

### Package Dependencies

```
verter-vscode (VS Code extension)
├── verter-lsp (Rust LSP binary, stdio)
│   ├── verter_host (file host + compilation)
│   ├── verter_diagnostics (lint rules + DiagnosticSet)
│   ├── verter_actions (quick fixes + refactoring)
│   └── TypeProvider (optional: TSGO or tsserver, for TS type checking)
├── @verter/language-shared (custom protocol types)
├── @verter/typescript-plugin (.vue import resolution, NAPI-backed)
└── @verter/oxc-bindings (OXC parser binary helper)

verter-mcp (MCP server binary, stdio + HTTP)
├── verter_host (file host + compilation)
├── verter_analysis (static analysis snapshots)
├── verter_diagnostics (lint rules + DiagnosticSet)
└── verter_actions (quick fixes + refactoring)

@verter/unplugin (universal bundler plugin)
└── @verter/native

@verter/playground (Netlify-hosted)
└── @verter/wasm (Rust template compiler, wasm-bindgen)
```

## Installation

### VS Code Extension

Install the Verter VS Code extension from the marketplace (coming soon).

### Manual Build (Fresh Machine)

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 2. Add WASM target and install wasm-pack
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# 3. Install pnpm (if not already installed, requires Node.js 18+)
corepack enable
corepack prepare pnpm@latest --activate

# 4. Clone and build
git clone https://github.com/pikax/verter.git
cd verter
pnpm install
pnpm build    # Builds: native bindings → LSP binary → WASM bindings → TypeScript packages

# 5. (Optional) Package VS Code extension
pnpm package
```

## Development

### Prerequisites

- **Node.js** 18+
- **pnpm** 10+
- **Rust** toolchain (for building native/WASM bindings) — install via [rustup](https://rustup.rs/)
- **wasm-pack** (for WASM builds) — `cargo install wasm-pack`
- **wasm32 target** — `rustup target add wasm32-unknown-unknown`

### Commands

```bash
# Build everything (sequential: native → lsp → wasm → TypeScript)
pnpm build

# Build individual layers
pnpm run build:native         # Rust → .node bindings
pnpm run build:lsp            # Rust → LSP binary (debug)
pnpm run build:lsp:release    # Rust → LSP binary (release, optimized)
pnpm run build:wasm           # Rust → .wasm bindings
pnpm run build:ts             # TypeScript packages

# Watch mode for extension development
pnpm watch

# Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm dev-extension

# Clean build artifacts
pnpm clean
```

### Rust Development

```bash
# Run all Rust tests
cargo test --workspace --verbose

# Run specific crate tests
cargo test --package verter_core

# Format and lint
cargo fmt --all
cargo clippy --workspace
```

## Testing

### Unit & Component Tests

```bash
# TypeScript tests (Vitest)
pnpm test
pnpm vitest --run                          # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts     # Specific file

# Rust tests
cargo test --workspace --verbose
cargo test --package verter_core test_name  # Specific test
```

Test files are co-located with source files as `*.spec.ts`.

### Benchmarks

See the [Performance](#performance) section above for latest results. To run benchmarks locally:

```bash
# Run benchmarks (8 fixtures + 20k file stress test)
pnpm --filter @verter/benchmark bench

# Run with JSON output (for CI)
pnpm --filter @verter/benchmark bench:json
```

Benchmarks are also triggered in CI via `/benchmark` PR comment.

### Profiling (hotpath + MCP)

Use hotpath profiling on real Vue projects (or fixture fallback) via `profile_ast`:

```bash
# Timing profile
pnpm run profile:hotpath

# Timing + allocation profile
pnpm run profile:hotpath:alloc

# MCP server for agents (serves http://localhost:6771/mcp)
pnpm run profile:hotpath:mcp
```

For MCP-capable agents, use [mcp/hotpath.mcp.json](./mcp/hotpath.mcp.json) as the server config.
If your agent expects a custom config path, point it at that file.
For client-specific setup examples, see [mcp/README.md](./mcp/README.md).

### Integration Tests

Tests Verter against real-world Vue projects (Vuetify, PrimeVue, etc.) to ensure compilation correctness:

```bash
# Manual trigger via GitHub Actions
# - Actions tab → Integration Test → Run workflow
# - Or comment "/integration" on any PR
```

See [.github/INTEGRATION_TEST.md](./.github/INTEGRATION_TEST.md) for details.

## Documentation

### Project Guides

- **[Documentation](https://verterjs.dev)** — Full documentation site
- **[Architecture Overview](https://verterjs.dev/guide/architecture)** — Deep dive into Verter's design
- **[Rust Setup Guide](https://verterjs.dev/contributing/rust-setup)** — Rust development environment
- **[Contributing Guide](https://verterjs.dev/contributing/)** — How to contribute
- **[CI/CD Documentation](https://verterjs.dev/contributing/ci-cd)** — Workflows and release process

### TypeScript Packages

| Package                     | README                                           | Description                       |
| --------------------------- | ------------------------------------------------ | --------------------------------- |
| `@verter/core`              | [README](./packages/core/README.md)              | SFC → TSX transformation engine   |
| `@verter/types`             | [README](./packages/types/readme.md)             | TypeScript utility types          |
| `@verter/native`            | [README](./packages/native/README.md)            | Native Node.js bindings (NAPI-RS) |
| `@verter/wasm`              | [README](./packages/wasm/README.md)              | WASM bindings for browser         |
| `@verter/unplugin`          | [README](./packages/unplugin/README.md)          | Universal bundler plugin          |
| `@verter/language-shared`   | [README](./packages/language-shared/readme.md)   | Shared protocol types             |
| `@verter/typescript-plugin` | [README](./packages/typescript-plugin/readme.md) | TypeScript plugin                 |
| `@verter/oxc-bindings`      | [README](./packages/oxc-bindings/readme.md)      | OXC parser helper                 |
| `verter-vscode`             | [README](./packages/vue-vscode/readme.md)        | VS Code extension                 |
| `@verter/playground`        | [README](./packages/playground/README.md)        | Online playground                 |

### Rust Crates

| Crate                | Description                                      |
| -------------------- | ------------------------------------------------ |
| `verter_core`        | Core template compiler                           |
| `verter_analysis`    | Static analysis: imports, exports, bindings       |
| `verter_host`        | In-memory file host: caching, dependency tracking |
| `verter_diagnostics` | Diagnostic engine: ~164 lint rules                |
| `verter_actions`     | Code actions: quick fixes, refactoring            |
| `verter_lsp`         | Rust LSP server binary (stdio)                   |
| `verter_mcp`         | MCP server binary (stdio + HTTP)                 |
| `verter_ffi`         | FFI types for NAPI/WASM boundaries               |
| `verter_span`        | Typed span types (Span, RelativeSpan, etc.)      |
| `verter_bench`       | Benchmarks and comparison examples               |
| `verter_napi`        | NAPI-RS Node.js bindings                         |
| `verter_wasm`        | WASM bindings (wasm-bindgen)                     |

## Credits

- [Svelte language-tools](https://github.com/sveltejs/language-tools) for proving inspiration
- [Vetur](https://github.com/vuejs/vetur) for providing the base for language support
- [Volar](https://github.com/vuejs/language-tools) for inspiration and testing

## License

MIT
