# verter_napi

> [!WARNING]
> This project is **experimental and under active development**. APIs, architecture, and package boundaries may change without notice.

NAPI-RS native binding crate that exposes [`verter_core`](../verter_core/) to Node.js. This is a thin FFI layer that compiles to a platform-specific `.node` binary (cdylib), consumed by the [`@verter/native`](../../packages/native/) npm package.

## Architecture

```mermaid
graph LR
    subgraph "Node.js"
        A["@verter/native<br/>(npm package)"] -->|loads| B["verter_napi.node<br/>(platform binary)"]
    end

    subgraph "Rust FFI Boundary"
        B -->|"#[napi] functions"| C["verter_napi<br/>(this crate)"]
        C -->|delegates to| D["verter_core<br/>(Rust compiler)"]
    end

    subgraph "Consumers"
        E["@verter/unplugin"] --> A
    end
```

### FFI Boundary Design

```mermaid
sequenceDiagram
    participant JS as Node.js (JavaScript)
    participant NAPI as verter_napi (FFI)
    participant Core as verter_core (Rust)

    JS->>NAPI: compile(input, options)
    Note over NAPI: Create OXC Allocator
    NAPI->>Core: generate(input, options, &allocator)
    Core-->>NAPI: CoreCodegenResult
    Note over NAPI: Convert UTF-8 offsets → UTF-16
    Note over NAPI: Drop OXC Allocator
    NAPI-->>JS: CodegenResult
```

Key constraints at the FFI boundary:

- **OXC allocator lifecycle** -- A fresh `oxc_allocator::Allocator` is created and dropped inside each FFI call. The allocator manages OXC AST memory and cannot safely cross the FFI boundary.
- **UTF-8 to UTF-16 conversion** -- Rust operates on UTF-8 byte offsets internally. At the NAPI boundary, `PositionResolver` converts all offsets to UTF-16 code units for JavaScript string compatibility.
- **NAPI object mapping** -- Rust structs are annotated with `#[napi(object)]` to generate JavaScript-compatible interfaces automatically.

## API

### Workspace (async filesystem access)

The `Workspace` class provides async filesystem access through the Rust VFS. All file I/O methods return Promises (run on the libuv thread pool).

```javascript
const { Workspace, VerterHost } = require("@verter/native");

// Create workspace rooted at project directory
const ws = new Workspace(["/path/to/project"]);

// Async file access
const content = await ws.readFile("/path/to/file.vue");
const exists = await ws.fileExists("/path/to/file.ts");
const isDirectory = await ws.isDir("/path/to/dir");
const entries = await ws.readDir("/path/to/dir"); // [{path, isDir}]
const files = await ws.walk("/path", ["node_modules"], [".vue", ".ts"]);

// Async file writes
await ws.writeFile("/path/to/file.ts", "export const x = 1;");
await ws.createDirAll("/path/to/new/dir");
await ws.deleteFile("/path/to/file.ts");
await ws.copyFile("/src/a.ts", "/dst/a.ts");

// Context-aware import resolution
const resolved = await ws.resolveImport("/src/App.vue", "./Child.vue");
const types = await ws.resolveImport("/src/App.vue", "pkg", "provider", "type");

// Project configuration (replaces auto-discovered graph)
ws.configureProjects([{
  root: "/project",
  workspaceRoot: "/project",
  compilerOptions: { baseUrl: ".", paths: { "@/*": ["src/*"] } },
}]);

// Create host backed by workspace
const host = VerterHost.withWorkspace({}, ws);
```

### Host (synchronous compilation)

All VerterHost methods are synchronous.

### `compile(input, options?) -> CodegenResult`

Compiles a Vue SFC string or Buffer into JavaScript with source maps.

```javascript
const { compile } = require("@verter/native");

const result = compile("<template><div>{{ msg }}</div></template>", {
  filename: "App.vue",
  includeSourceContent: true,
  isProduction: false,
  ssr: false,
  features: {
    optionsApi: true,
    propsDestructure: true,
  },
});

const bufferResult = compile(Buffer.from("<template><div>{{ msg }}</div></template>"));

console.log(result.code); // compiled JavaScript
console.log(result.sourceMap); // source map JSON string
```

### `compileSync(input, options?) -> CodegenResult`

Identical to `compile` -- kept for API compatibility (accepts string or Buffer).

### `compileForVite(input, options?) -> ViteCodegenResult`

Vite-optimized compilation returning split blocks for virtual module serving. Each block includes import metadata with UTF-16 offsets for JavaScript string manipulation.

```javascript
const { compileForVite } = require("@verter/native");

const result = compileForVite(sfcSource, {
  filename: "App.vue",
  isProduction: false,
  ssr: false,
  sourcemap: true,
});

// result.script   -- { code, sourceMap, imports, bodyStartUtf16 } | null
// result.template -- { code, sourceMap, imports, bodyStartUtf16 } | null
// result.styles   -- [{ code, sourceMap, scoped, isModule, lang, moduleName, moduleClasses }]
// result.durationMs -- compilation time in milliseconds
```

### Type Definitions

All compile entry points accept `input` as `string | Buffer`.

| NAPI Type            | Fields                                                                                     |
| -------------------- | ------------------------------------------------------------------------------------------ |
| `FeatureFlags`       | `optionsApi?: boolean`, `propsDestructure?: boolean`                                       |
| `CodegenOptions`     | `filename?`, `includeSourceContent?`, `ssr?`, `isProduction?`, `componentId?`, `features?` |
| `CodegenResult`      | `code`, `sourceMap`, `codeWithSourceMap`                                                   |
| `ViteCodegenOptions` | `filename?`, `ssr?`, `isProduction?`, `componentId?`, `sourcemap?`                         |
| `ViteCodegenResult`  | `script?`, `template?`, `styles[]`, `durationMs`                                           |
| `JsBlockOutput`      | `code`, `sourceMap?`, `imports[]`, `bodyStartUtf16`                                        |
| `JsBlockImport`      | `source`, `specifiers[]`, `startUtf16`, `endUtf16`                                         |
| `JsStyleBlock`       | `code`, `sourceMap?`, `scoped`, `isModule`, `lang?`, `moduleName?`, `moduleClasses[]`      |

## Build

### Prerequisites

- Rust toolchain (stable)
- Node.js (for NAPI-RS build tools)

### Building the Native Binary

From the `packages/native/` directory (which orchestrates the NAPI build):

```bash
napi build -o dist --platform --release --manifest-path ../../crates/verter_napi/Cargo.toml
```

Or from the repository root:

```bash
pnpm run build:native
```

This produces a platform-specific `.node` file (e.g., `verter_napi.linux-x64-gnu.node`).

### Platform Targets

The CI builds native binaries for 7 platform targets:

| Platform            | Target Triple                |
| ------------------- | ---------------------------- |
| Linux x64 (glibc)   | `x86_64-unknown-linux-gnu`   |
| Linux x64 (musl)    | `x86_64-unknown-linux-musl`  |
| Linux arm64 (glibc) | `aarch64-unknown-linux-gnu`  |
| Linux arm64 (musl)  | `aarch64-unknown-linux-musl` |
| macOS x64           | `x86_64-apple-darwin`        |
| macOS arm64         | `aarch64-apple-darwin`       |
| Windows x64         | `x86_64-pc-windows-msvc`     |

### build.rs

The build script calls `napi_build::setup()` to configure NAPI-RS code generation.

## Dependencies

| Crate           | Purpose                                           |
| --------------- | ------------------------------------------------- |
| `verter_core`   | Core Rust template compiler                       |
| `oxc_allocator` | Memory allocator for OXC AST (created per-call)   |
| `napi` (v2)     | NAPI-RS runtime (features: `napi8`, `serde-json`) |
| `napi-derive`   | Procedural macros for `#[napi]` annotations       |
| `napi-build`    | Build-time NAPI-RS setup                          |

## License

ISC
