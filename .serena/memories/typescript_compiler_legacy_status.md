# TypeScript Vue-to-TSX Compiler Legacy Status

The TypeScript Vue-to-TSX compiler path (`@verter/core`, `packages/core`) is an old implementation. It has been superseded by the Rust-based compiler/host/session pipeline for current Vue compilation, IDE TSX codegen, and component-meta semantics.

Evidence from git history:
- Early history starts with TypeScript package/compiler work (`init`, `support for v-for`, `support for bindings`, `add vscode extension`, `added lsp`, etc.).
- `f5be93d6` (`feat(verter): add vue tokenizer in rust`, 2026-01-22) introduces the Rust tokenizer/compiler crate substrate.
- `ed3ff347` (`refactor(compiler): use ast based compiler instead of events`, 2026-02-19) adds the Rust AST-based compiler implementation and large Rust template codegen structure.
- `60da706a` (`feat(core): add TSX code generation module for IDE type checking`, 2026-02-24) adds Rust-side TSX codegen under the compiler/host/NAPI path.
- `7b716208` (`docs(*): rewrite README to reflect Rust-first toolchain`, 2026-03-12) explicitly rewrites docs toward the Rust-first toolchain and de-emphasizes legacy TS packages. Current `README.md` describes `packages/core/` as `@verter/core — Legacy SFC→TSX transformer (internal)` and says it predates Rust IDE codegen.
- `e7520f41` (`refactor(*): split verter_core into verter_parser + verter_compiler`, 2026-03-31) establishes the current parser/compiler crate split.
- `306b10d6` (`refactor(*): native type solver cutover — delete legacy evaluator`, 2026-04-02) makes the native solver the authority for type expansion used by component-meta and macro surfaces.
- `78201307` (`refactor(*): cut component-meta over to native graph resolution`, 2026-04-06) removes TS/TSGO-backed component-meta expansion paths in favor of native graph resolution.
- `95c34fc9` (`refactor(meta): native @defaultValue tag + expanded-prop JSDoc + delete TS compat schema/theme/JSDoc forks`, 2026-04-23) deletes TS compat semantic forks and states Rust is the authority for compat semantics.
- `5bd95efb` (`refactor(napi): cutover compileBatch bypass to host-backed compileMany`, 2026-05-01) removes direct NAPI `verter_compiler::compile` batch bypasses and routes batch SFC compile through `VerterHost::compile_many`, scheduler, dispatch, and compile cache.

Practical guidance:
- Treat Rust compiler/session/semantic code as the current authority for Vue compilation, IDE TSX generation, and component-meta behavior.
- Do not assume `packages/core` behavior represents current architecture unless a task explicitly targets legacy/internal compatibility.
- For compiler/codegen work, start from `crates/verter_compiler`, `crates/verter_session`, and `.claude/skills/compiler-codegen/SKILL.md`.
- If touching old TypeScript Vue-to-TSX code, first verify whether the relevant package/consumer still uses it. Prefer deleting or documenting legacy paths when an approved plan calls for cleanup.
- Caveat: some older docs still describe `@verter/core` as an active TypeScript pipeline (`docs/guide/*`, `docs/api/core.md`, `packages/core/README.md`). Prefer the current root `README.md`, `CLAUDE.md`, and domain skills for architectural authority unless updating those stale docs is the task.