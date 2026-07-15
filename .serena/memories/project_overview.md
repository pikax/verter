# Project Overview

Verter is a Vue compiler and Language Server Protocol implementation. It converts Vue Single File Components (SFCs) to valid TSX for TypeScript-powered IDE/type checking and compiles templates to optimized render functions. Unlike Volar-style virtual-file approaches, Verter aims to generate actual valid TSX code.

The repository is a hybrid Rust + TypeScript monorepo:
- Rust crates own parsing/compilation, semantic analysis, host/session caching, LSP/MCP binaries, NAPI native bindings, and wasm-bindgen WASM output.
- TypeScript packages own utility types, bundler/plugin integration, VS Code/client glue, docs, benchmarks, playground, and distribution adapters. The Rust compiler owns both IDE-oriented source generation and runtime code generation.

Top-level structure:
- `crates/`: Rust workspace crates such as `verter_compiler`, `verter_semantic`, `verter_session`, `verter_lsp`, `verter_mcp`, `verter_napi`, `verter_wasm`, `verter_tsc`, `verter_protocol`, `verter_span`, and related support crates.
- `packages/`: pnpm workspace packages including `@verter/component-meta`, `@verter/native`, `@verter/wasm`, `@verter/unplugin`, `@verter/types`, `@verter/language-shared`, `@verter/typescript-plugin`, `@verter/playground`, `@verter/benchmark`, `@verter/proto`, `@verter/verter-tsc`, `verter-vscode`, and private behavioral-test packages.
- `extensions/`: VS Code / TypeScript extension packaging areas retained alongside the package workspace.
- `docs/`: VitePress documentation, API docs, architecture docs, guides, migration and contributing docs.
- `mcp/`: Verter MCP server configs and docs.
- `examples/`, `scripts/`, `.integration-tests/`, `.claude/skills/`: examples, automation scripts, optional integration corpora, and shared project reference docs.

Canonical project docs:
- `CLAUDE.md` is the canonical high-level architecture/build/test/agent reference, despite the historical filename.
- `AGENTS.md` is the neutral entry point and routes tasks to the shared docs.
- `.claude/skills/*/SKILL.md` contains domain-specific guidance. Load only the relevant skill for the current task.
