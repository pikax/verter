# Rust Setup

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Guide for setting up Rust development for Verter.

## Prerequisites

- **Rust stable toolchain** via [rustup](https://rustup.rs/)
- **wasm-pack** for WASM builds: `cargo install wasm-pack`

Verify your installation:

```bash
rustc --version
cargo --version
wasm-pack --version   # only needed for WASM builds
```

## Project Structure

All Rust crates are in the `crates/` directory:

| Crate | Purpose |
|-------|---------|
| `verter_core` | Template compiler -- tokenizer, parser, AST, script processing, template codegen (VDOM + Vapor), TSX generation, CSS processing |
| `verter_analysis` | Static analysis -- imports, exports, bindings, type resolution, CSS selector parsing, template element analysis |
| `verter_host` | File host -- in-memory caching, dependency tracking, multi-file compilation |
| `verter_scheduler` | Async file scheduler -- per-file Source→Analysis→Artifact stages, priority queue, blocker registry |
| `verter_diagnostics` | Diagnostic engine -- Vue SFC lint rules, rule trait, visitor, diagnostic set |
| `verter_actions` | Code actions engine -- quick fixes, refactoring (depends on `verter_diagnostics` + `verter_analysis`) |
| `verter_lsp` | LSP server binary -- stdio transport, feature handlers, document synchronization, TypeProvider integration (TSGO + tsserver) |
| `verter_ffi` | FFI types -- shared serializable structs for NAPI and WASM boundaries |
| `verter_napi` | NAPI-RS bindings -- Node.js native addon (cdylib) |
| `verter_wasm` | wasm-bindgen bindings -- browser WASM module (cdylib) |
| `verter_bench` | Benchmarks -- comparison examples against Vue's official compiler |

### Dependency Graph

```
verter_core (no deps on other verter crates)
    |
    +-- verter_analysis (depends on verter_core)
    |       |
    |       +-- verter_host (depends on verter_core + verter_analysis + verter_scheduler[optional])
    |       |       |
    |       |       +-- verter_lsp (depends on verter_host + verter_scheduler + verter_diagnostics + verter_actions)
    |
    +-- verter_scheduler (depends on verter_span only — domain-agnostic)
    |       |
    |       +-- verter_diagnostics (depends on verter_analysis)
    |       |
    |       +-- verter_actions (depends on verter_diagnostics + verter_analysis)
    |
    +-- verter_ffi (depends on verter_core)
    |       |
    |       +-- verter_napi (depends on verter_ffi + verter_host)
    |       |
    |       +-- verter_wasm (depends on verter_ffi + verter_core)
    |
    +-- verter_bench (depends on verter_core)
```

## Building

```bash
# Build all crates (debug)
cargo build --workspace

# Build native NAPI bindings (release, for use by TypeScript packages)
cargo build --release --package verter_napi

# Build LSP binary (debug, for F5 extension development)
cargo build -p verter_lsp

# Build LSP binary (release, optimized)
cargo build --release -p verter_lsp

# Build WASM (via wasm-pack)
wasm-pack build crates/verter_wasm --target web
```

### Quick Rebuild for Native Bindings

When iterating on Rust code used by TypeScript packages:

```bash
# Build and copy native binary (Windows example)
cargo build --release --package verter_napi && \
  rm -f packages/native/dist/verter-native.win32-x64-msvc.node && \
  cp target/release/verter_napi.dll packages/native/dist/verter-native.win32-x64-msvc.node
```

Or use the project's build scripts:

```bash
pnpm run build:native    # Build + copy native bindings
pnpm run build:lsp       # Build LSP binary (debug)
pnpm run build:wasm      # Build WASM + copy to playground
```

## Testing

```bash
# Run all Rust tests
cargo test --workspace --verbose

# Run tests for a specific crate
cargo test --package verter_core --verbose

# Run a specific test by name
cargo test --package verter_core test_name

# Run tests with output (useful for debugging)
cargo test --package verter_core -- --nocapture
```

## TDD Required

**Test-Driven Development is mandatory for all Rust changes.**

1. **Write failing tests first** -- before implementing any feature or fix, write tests that demonstrate the expected behavior and verify they fail
2. **Implement the minimum code** to make the failing tests pass
3. **Refactor** while keeping tests green

### Assertion Requirements

Every test must verify both what SHOULD be present AND what should NOT be present:

```rust
// GOOD: Both positive and negative assertions
let result = compile(input);
assert!(result.contains("_createElementVNode"), "should emit vdom call");
assert!(!result.contains("v-if"), "v-if must not appear in output");

// BAD: Only positive -- passes even if output contains broken content
let result = compile(input);
assert!(result.contains("_createElementVNode"), "should emit vdom call");
```

### Codegen Test Pattern

All codegen tests must validate that the output is syntactically valid JavaScript using the OXC parser:

```rust
use crate::test_utils::gen_and_validate;

#[test]
fn test_my_feature() {
    let result = gen_and_validate(r#"<template><div>hello</div></template>"#);
    assert!(result.contains("expected output"));
    assert!(!result.contains("unexpected content"));
}
```

## Code Quality

Run these before committing:

```bash
# Lint with clippy (treat warnings as errors)
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings

# Format all Rust code
cargo fmt --all
```

## Key Modules

When working on specific areas, these are the primary entry points:

| Area | Entry Point |
|------|-------------|
| Compilation pipeline | `crates/verter_core/src/compile.rs` |
| SFC tokenizer | `crates/verter_core/src/tokenizer/byte.rs` |
| Template AST | `crates/verter_core/src/ast/types.rs` |
| VDOM codegen | `crates/verter_core/src/template/code_gen/vdom/` |
| Vapor codegen | `crates/verter_core/src/template/code_gen/vapor/` |
| TSX codegen (LSP) | `crates/verter_core/src/ide/template/mod.rs` |
| Script processing | `crates/verter_core/src/script/process.rs` |
| CSS processing | `crates/verter_core/src/css/mod.rs` |
| Static analysis | `crates/verter_analysis/src/lib.rs` |
| LSP server | `crates/verter_lsp/src/server.rs` |
| TSGO type provider | `crates/verter_lsp/src/tsgo/ipc.rs` |
| tsserver type provider | `crates/verter_lsp/src/tsserver/ipc.rs` |
| Diagnostics | `crates/verter_diagnostics/src/lib.rs` |

## Two Template Codegen Paths

The Rust compiler has two separate template codegen paths. Modifying one does NOT affect the other:

| Path | Module | Purpose | Output |
|------|--------|---------|--------|
| VDOM/Vapor | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| IDE | `ide/template/` | Valid JSX/TSX for LSP/TSGO type checking | `<div prop={expr}>` JSX elements |

The LSP uses the TSX path. Changes to VDOM codegen do not affect LSP hover, completions, or diagnostics.
