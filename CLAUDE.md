# Verter

Verter is a Vue compiler and Language Server Protocol (LSP) implementation. It converts Vue Single File Components (SFCs) to valid TSX (leveraging TypeScript for type checking) and compiles templates to optimized render functions. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

The project is a hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter_lsp` binary, communicates over stdio), while TypeScript packages handle the SFC-to-TSX transformation and IDE integration.

## Architecture

### Package Dependency Graph

```
verter-vscode (VS Code extension)
├── verter-lsp (Rust LSP binary, stdio)
│   ├── verter_host (file host + compilation)
│   ├── verter_diagnostics (lint rules + DiagnosticSet)
│   ├── verter_actions (quick fixes + refactoring)
│   └── TsgoTypeProvider (optional, for TS type checking)
├── @verter/language-shared (custom protocol types)
├── @verter/typescript-plugin (.vue import resolution, NAPI-backed)
└── @verter/unplugin (bundler plugin)
    └── @verter/native

verter-mcp (MCP server binary, stdio + HTTP)
├── verter_host (file host + compilation)
├── verter_analysis (static analysis snapshots)
├── verter_diagnostics (lint rules + DiagnosticSet)
└── verter_actions (quick fixes + refactoring)

@verter/playground (Netlify-hosted)
└── @verter/wasm (Rust template compiler, wasm-bindgen)
```

### Repository Structure

```
crates/
  verter_core/       # Core template compiler (Rust)
  verter_analysis/   # Static analysis: imports, exports, bindings, type resolution
  verter_host/       # In-memory file host: caching, dependency tracking, multi-file compilation
  verter_diagnostics/ # Vue SFC diagnostic engine: rule trait, visitor, diagnostics (depends only on verter_analysis)
  verter_actions/    # Code actions engine: quick fixes, refactoring (depends on verter_diagnostics + verter_analysis)
  verter_lsp/        # Rust LSP server binary (stdio, launched by VS Code extension)
  verter_ffi/        # FFI types: shared serializable structs for NAPI/WASM boundaries
  verter_bench/      # Benchmarks and comparison examples (Rust)
  verter_mcp/        # MCP server binary: analysis, diagnostics, scoring for AI agents
  verter_napi/       # Native Node.js bindings (NAPI-RS cdylib)
  verter_wasm/       # WASM bindings (wasm-bindgen cdylib)
packages/
  core/              # @verter/core - SFC parser & TSX transformer
  types/             # @verter/types - TypeScript utility types
  native/            # @verter/native - Native binding loader + platform packages
  wasm/              # @verter/wasm - WASM binding wrapper
  unplugin/          # @verter/unplugin - Universal bundler plugin
  language-shared/   # @verter/language-shared - Shared LSP protocol types
  typescript-plugin/ # @verter/typescript-plugin - TS language service plugin
  oxc-bindings/      # @verter/oxc-bindings - OXC parser binary helper
  playground/        # @verter/playground - Online playground (private, Netlify-hosted)
  vue-vscode/        # verter-vscode - VS Code extension
  example/           # Example project
scripts/
  check-versions.mjs # Version check + publish order for CI
```

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has **two separate template codegen paths**. Modifying one does NOT affect the other:

| Path | Module | Purpose | Output |
|------|--------|---------|--------|
| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| **TSX** | `tsx/template/` | Valid JSX for LSP/TSGO type checking | `<div prop={expr}>` JSX elements |

The **LSP uses the TSX path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this TSX output. Changes to VDOM codegen do NOT affect LSP hover/completions.

### Cached Directive Fields on ElementNode

The parser extracts structural directives from `el.props` via `prop.take()` and caches them as dedicated fields on `ElementNode` (`ast/types.rs`):

| Field | Directive | In `el.props`? | Notes |
|-------|-----------|----------------|-------|
| `v_condition` | `v-if`, `v-else-if`, `v-else` | **No** (taken) | Contains `ElementNodeCondition` with kind + prop |
| `v_for` | `v-for` | **No** (taken) | Contains the full `NodeProp` |
| `v_slot` | `v-slot`, `#name` | **No** (taken) | Contains the full `NodeProp` |
| `v_once` | `v-once` | **No** (taken) | Contains the full `NodeProp` |
| `v_ref` | `ref`, `:ref` | **No** (taken) | Contains the full `NodeProp` |

**Consequence**: Code iterating `el.props` will **never see** these directives. Both codegen paths must handle them explicitly. The TSX module removes `v-if/v-for/v-slot/v-once` attributes (they become JSX wrappers/removals) and converts `ref` to JSX expression syntax (`ref={"name"}`).

### Position Encoding (CRITICAL rules)

See `/position-encoding` skill for full span type reference, encoding tables, and path normalization details.

**Line/Column Base Rules** (off-by-one bugs):
- **PositionResolver is 1-based** — subtract 1 for source maps and LSP
- **Source maps, LSP, VS Code are all 0-based**
- **OXC/verter spans are byte offsets** — no line/column conversion needed

**Serialization rule**: All data crossing serde/MCP/LSP/FFI boundaries MUST use `Span` (SFC-absolute). `RelativeSpan`/`PartialGeneratedSpan`/`GeneratedSpan` do not implement Serialize.

### Path Normalization (CRITICAL rules)

See `/position-encoding` skill for canonical ID format and boundary tables.

1. **Receive → normalize immediately** (`canonicalize_id()` or `uri_to_canonical_id_from_str()`)
2. **Store only canonical IDs** in all maps and caches
3. **Denormalize at exit boundaries** (file:// URIs or OS paths)
4. **Never compare raw paths** — always compare canonical IDs

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → lsp → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:lsp            # Build Rust LSP binary (debug)
pnpm run build:lsp:release    # Build Rust LSP binary (release, optimized)
pnpm run build:mcp            # Build MCP server binary (debug)
pnpm run build:mcp:release    # Build MCP server binary (release, optimized)
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then LSP binary (shares compiled Rust deps with native, avoids recompilation), then WASM (needed by playground), then all TS packages.

See `/build-and-profiling` skill for build dependency chains, rebuild sequences, and profiling setup.

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm clean                    # Remove build artifacts
```

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust
cargo test --workspace --verbose             # All Rust tests
cargo test --package verter_core test_name   # Specific Rust test
cargo test --package verter_core 2>&1 | tail -60  # Full suite with truncated output
```

### End-of-change Checks

Run these after making changes:

```bash
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
```

### Testing Requirements

**IMPORTANT — TDD (Test-Driven Development) is mandatory**:
1. **Write failing tests first** — before implementing any feature or fix, write one or more tests that demonstrate the expected behavior and verify they fail
2. **Implement the minimum code** to make the failing tests pass
3. **Refactor** if needed while keeping tests green

Coverage expectations:
- New features: Add tests covering the new functionality
- Bug fixes: Add tests that would have caught the bug
- Refactoring: Ensure existing tests pass and add tests for edge cases discovered
- Behavioral changes: Add tests verifying the new behavior

Tests serve as documentation of expected behavior and prevent regressions.

**IMPORTANT — Always include negative assertions**:

Every test must verify both what SHOULD be present AND what should NOT be present. A test that only checks for expected output can pass even when the output contains invalid/broken content alongside the expected content.

```rust
// GOOD: Both positive and negative assertions
let result = gen_tsx_template(r#"<template><div v-if="show">hello</div></template>"#);
assert!(result.contains("if(show)"), "should have IIFE if-block condition");  // positive
assert!(!result.contains("v-if"), "v-if attribute must be removed from JSX"); // negative

// BAD: Only positive assertion — passes even if v-if="show" leaks into output
let result = gen_tsx_template(r#"<template><div v-if="show">hello</div></template>"#);
assert!(result.contains("if(show)"), "should have IIFE if-block condition"); // not enough!
```

For codegen tests: always verify that removed/transformed Vue syntax does NOT appear in output. For type tests: always include both positive assertions and `@ts-expect-error` negative assertions to guard against `any`/`never`.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, E2E best practices, and server cleanup.

## Dependencies Policy

- Keep dependencies at their latest versions
- Rust deps: update in `Cargo.toml`, run `cargo update`
- JS deps: `pnpm up -r -i -L` to interactively update all
- `workspace:^` deps are rewritten by `pnpm publish` automatically

## Commit Convention

This project uses **conventional commits** for automatic changelog generation via [git-cliff](https://git-cliff.org/).

```
<type>(<scope>): <description>

Types:
  feat     - New feature
  fix      - Bug fix
  perf     - Performance improvement
  refactor - Code refactoring (no behavior change)
  docs     - Documentation only
  test     - Adding/updating tests
  chore    - Build, CI, tooling changes
  release  - Version bump and release

Scopes:
  core     - verter_core Rust crate
  napi     - verter_napi / @verter/native
  wasm     - verter_wasm / @verter/wasm
  play     - playground
  unplugin - @verter/unplugin
  lsp      - language-server
  types    - @verter/types
  ts       - @verter/core (TypeScript)
  ci       - CI/CD workflows
  *        - multiple areas

Examples:
  feat(core): add v-memo directive support
  fix(wasm): correct memory leak in compile()
  chore(ci): add nightly WASM build workflow
  release(all): v0.0.1-alpha.1
```

## CI/CD

See [.claude/ci-cd.md](.claude/ci-cd.md) for detailed CI/CD documentation including:

- Workflow specifications (CI, nightly, release)
- Pre-release versioning flow (alpha → beta → rc → stable)
- Publishing process (npm + crates.io)
- Nightly WASM builds and playground deployment
- Required GitHub secrets configuration

## Skills Reference

Detailed reference material is available as on-demand skills (loaded automatically when relevant):

| Skill | Use When |
|-------|----------|
| `/architecture` | Working on any specific module, need key files, type tables, LSP features, plugin system, analysis types |
| `/position-encoding` | Working with spans, positions, coordinate conversions, path normalization details |
| `/build-and-profiling` | Debugging build order, rebuild sequences, profiling, MCP server setup |
| `/testing` | Writing tests, test patterns, sourcemap testing, E2E workflow, server cleanup |
| `/rust-performance` | Optimizing Rust code, allocation patterns, batch operations, CodeTransform API |
