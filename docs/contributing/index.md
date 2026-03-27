# Contributing

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

How to contribute to Verter.

## Prerequisites

- **Node.js** 18+ and **pnpm** 9+
- **Rust** stable toolchain via [rustup](https://rustup.rs/)
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
pnpm test                              # All JS/TS tests
pnpm vitest --run                      # All tests, non-watch mode
pnpm vitest --run path/to/test.ts      # Specific file

# Rust
cargo test --workspace --verbose       # All Rust tests
cargo test --package verter_core test  # Specific Rust test
```

See the [Testing Guide](./testing.md) for detailed testing patterns and requirements.

## Code Style

Run these checks after making changes:

```bash
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
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
| `core`     | `verter_core` Rust crate         |
| `napi`     | `verter_napi` / `@verter/native` |
| `wasm`     | `verter_wasm` / `@verter/wasm`   |
| `play`     | Playground                       |
| `unplugin` | `@verter/unplugin`               |
| `lsp`      | Language server                  |
| `types`    | `@verter/types`                  |
| `ts`       | `@verter/core` (TypeScript)      |
| `ci`       | CI/CD workflows                  |
| `*`        | Multiple areas                   |

### Examples

```
feat(core): add v-memo directive support
fix(lsp): correct hover position for multi-line expressions
perf(core): batch mutation passes in template codegen
refactor(ts): simplify macro plugin architecture
docs: update contributing guide
test(core): add v-for key validation tests
chore(ci): add nightly WASM build workflow
release(all): v0.0.1-alpha.3
```

## Repository Structure

```
crates/          # Rust crates (compiler, LSP, analysis, FFI)
packages/        # TypeScript packages (core, types, unplugin, extension)
scripts/         # CI/CD and utility scripts
docs/            # Documentation (VitePress)
```

See the [Rust Setup](./rust-setup.md) guide for details on the Rust crate structure.

## PR Checklist

Before submitting a pull request:

- [ ] Tests added or updated covering the change
- [ ] Documentation updated if applicable
- [ ] No TypeScript errors (`pnpm build:ts` succeeds)
- [ ] Rust checks pass (`cargo clippy`, `cargo fmt`, `cargo test`)
- [ ] Code style is consistent with surrounding code
- [ ] Conventional commit message used
- [ ] Both positive and negative assertions in tests (see [Testing Guide](./testing.md))
