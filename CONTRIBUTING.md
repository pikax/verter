# Contributing to Verter

Thank you for your interest in contributing to Verter! This guide will help you get started.

## Getting Started

### Prerequisites

- Node.js 18+
- pnpm 10+
- Rust stable toolchain with `rustfmt`, `clippy`, and `cargo-nextest`
- VS Code (for extension development)

### Setup

```bash
# Clone the repository
git clone https://github.com/pikax/verter.git
cd verter

# Install dependencies
pnpm install

# Build all packages
pnpm build
```

## Development Workflow

### Building

```bash
# Build all packages (respects dependency order)
pnpm build

# Build a specific package
pnpm --filter @verter/typescript-plugin build

# Watch mode
pnpm watch

# Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm dev-extension
```

### Testing

Verter uses Vitest for JavaScript/TypeScript packages and Cargo tests for the
Rust workspace. The canonical Rust gate is a pair: Nextest covers every
workspace test target, while the second command preserves the shared-process
`verter_session` integration surface.

```bash
# Run every package-owned JS/TS test script
pnpm test

# Run one package or test file during iteration
pnpm --filter @verter/typescript-plugin test
pnpm exec vitest run path/to/file.spec.ts

# Canonical Rust pair
cargo nextest run --workspace
cargo test -p verter_session --tests

# Targeted Rust iteration
cargo test -p verter_compiler test_name
```

### Test Patterns

JavaScript/TypeScript tests are normally co-located as `*.spec.ts`. Rust unit
tests live beside their owner; larger behavioral surfaces use the owning
crate's integration-test layout. Start with a failing behavioral or typed
assertion, implement the smallest complete fix, then rerun the affected suite
before refactoring. Do not use source-text checks as substitutes for product
behavior when a typed or executable assertion is possible.

## Project Architecture

### Package Structure

```
crates/
├── verter_lsp/            # Rust LSP server binary (stdio)
├── verter_compiler/       # Runtime and IDE code generation
├── verter_session/        # Host, semantic graph, caches, and sessions
└── ...

packages/
├── native/                # @verter/native bindings and loader
├── types/                 # @verter/types declarations
├── language-shared/       # Shared client/server protocol types
├── typescript-plugin/     # Editor-owned TypeScript integration
├── unplugin/              # Universal bundler integration
└── vue-vscode/            # VS Code extension
```

### Dependency Graph

```
verter-vscode
├── verter-lsp (Rust LSP binary, stdio)
├── @verter/language-shared
└── @verter/typescript-plugin
    ├── @verter/language-shared
    └── @verter/native
```

### Compiler and IDE Pipeline

```
Vue SFC → verter_compiler + verter_session ┬→ runtime JavaScript/CSS
                                            └→ IDE TSX → TSGO/tsserver
```

Rust owns parsing, semantic resolution, runtime code generation, and IDE TSX generation. TypeScript packages own editor/provider integration, protocol bindings, and bundler orchestration.

## Code Patterns

### Type Helper Prefixes

Generated IDE helpers use reserved Verter prefixes so user-facing output can distinguish or hide implementation-only declarations.

- `___VERTER___` - Internal IDE-codegen helpers
- `$V_` - Collision-resistant identifiers in string-exported type declarations

### Source mappings

Use the owning Rust `CodeTransform`/source-map APIs for compiler output and the existing package-local mapping utilities for TypeScript adapters. Every generated-code change needs behavioral syntax validation and source-map assertions.

## Pull Request Process

1. **Fork** the repository
2. **Create** a feature branch: `git checkout -b feature/my-feature`
3. **Make** your changes
4. **Test** the affected packages/crates, then run the canonical gates relevant
   to the change
5. **Commit** with clear messages
6. **Push** to your fork
7. **Open** a Pull Request

### Commit Messages

Use clear, descriptive commit messages:

```
feat(compiler): add support for a template transform
fix(lsp): resolve completion for template refs
docs(readme): update installation instructions
test(macros): add tests for withDefaults
```

### PR Checklist

- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] No TypeScript errors
- [ ] Rust format and warning-denied Clippy pass for affected crates
- [ ] Required package and Rust behavioral tests pass
- [ ] Follows existing code style

## Debugging

### VS Code Extension

1. Open monorepo in VS Code
2. Run `pnpm run build:lsp` to build the Rust LSP binary
3. Run "Launch Client" debug configuration (F5)
4. New window opens with extension loaded — the extension spawns the `verter-lsp` binary over stdio

### TypeScript Plugin

Plugin logs go to TypeScript's server log. In VS Code:

1. Open command palette
2. "TypeScript: Open TS Server Log"

## Questions?

- Open an issue for bugs or feature requests
- Check existing issues and PRs first
- Include reproduction steps for bugs

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
