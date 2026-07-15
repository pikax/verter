# Contributing

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

How to contribute to Verter.

## Prerequisites

- **Node.js** 18+ and **pnpm** 9+
- **Rust** stable toolchain via [rustup](https://rustup.rs/), including
  `rustfmt`, `clippy`, and `cargo-nextest`
- **VS Code** (for extension development)

## Setup

```bash
git clone https://github.com/pikax/verter.git
cd verter
pnpm install
pnpm build
```

`pnpm build` runs sequentially: native bindings, LSP binary, WASM, then TypeScript packages. This ensures all downstream consumers have their dependencies available.

## Development

```bash
pnpm watch            # Watch-build TS packages for extension development
pnpm dev-extension    # Build LSP binary, then watch language-shared + extension + TypeScript plugin
```

To test the extension locally, press **F5** in VS Code after building to launch the Extension Development Host.

## Testing

```bash
# TypeScript / JavaScript
pnpm test                                      # Every package-owned test script
pnpm --filter @verter/typescript-plugin test  # One package
pnpm exec vitest run path/to/test.ts           # One test file

# Rust
cargo nextest run --workspace              # Every workspace test target
cargo test -p verter_session --tests       # Shared-process session surface
cargo test -p verter_compiler test_name    # Targeted iteration
```

See the [Testing Guide](./testing.md) for detailed testing patterns and requirements.

## Code Style

Run these checks after making changes:

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

TypeScript formatting follows the project's existing style. There is no separate formatter command -- maintain consistency with surrounding code.

## Commit Convention

This project uses **conventional commits** for automatic changelog generation via [git-cliff](https://git-cliff.org/):

```
<type>(<scope>): <description>
```

### Types

| Type       | Description                           |
| ---------- | ------------------------------------- |
| `feat`     | New feature                           |
| `fix`      | Bug fix                               |
| `perf`     | Performance improvement               |
| `refactor` | Code refactoring (no behavior change) |
| `docs`     | Documentation only                    |
| `test`     | Adding or updating tests              |
| `chore`    | Build, CI, or tooling changes         |
| `release`  | Version bump and release              |

### Scopes

| Scope      | Area                             |
| ---------- | -------------------------------- |
| `compiler` | `verter_compiler` Rust crate     |
| `napi`     | `verter_napi` / `@verter/native` |
| `wasm`     | `verter_wasm` / `@verter/wasm`   |
| `play`     | Playground                       |
| `unplugin` | `@verter/unplugin`               |
| `lsp`      | Language server                  |
| `types`    | `@verter/types`                  |
| `ci`       | CI/CD workflows                  |
| `*`        | Multiple areas                   |

### Examples

```
feat(compiler): add v-memo directive support
fix(lsp): correct hover position for multi-line expressions
perf(core): batch mutation passes in template codegen
refactor(session): simplify semantic query ownership
docs: update contributing guide
test(compiler): add v-for key validation tests
chore(ci): add nightly WASM build workflow
release(all): v0.0.1-beta.1
```

## Repository Structure

```
crates/          # Rust crates (compiler, semantic/session, LSP, FFI)
packages/        # TypeScript adapters, types, integrations, and clients
scripts/         # CI/CD and utility scripts
docs/            # Documentation (VitePress)
```

See the [Rust Setup](./rust-setup.md) guide for details on the Rust crate structure.

## PR Checklist

Before submitting a pull request:

- [ ] Tests added or updated covering the change
- [ ] Documentation updated if applicable
- [ ] No TypeScript errors (`pnpm build:ts` succeeds)
- [ ] Rust checks pass (`cargo fmt --all --check`, warning-denied Clippy, and
      the canonical Nextest/session pair where applicable)
- [ ] Code style is consistent with surrounding code
- [ ] Conventional commit message used
- [ ] Both positive and negative assertions in tests (see [Testing Guide](./testing.md))
