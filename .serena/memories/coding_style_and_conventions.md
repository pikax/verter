# Coding Style and Conventions

General:
- Follow TDD for every code change: write a failing test first, implement the minimum fix, rerun tests, then refactor while green.
- Prefer architecturally correct, long-term fixes. Do not use timing, migration breadth, or perceived effort as reasons to weaken an approved design.
- Avoid shims, dual paths, compatibility wrappers, and feature flags when replacing a system unless the plan explicitly requires them.
- No stub tests or placeholder implementations in landed code. Tests must discriminate: they should fail against the buggy/pre-change behavior and pass after the fix.
- Do not add phase/cutover/project-management archaeology in production comments. Durable architecture belongs in docs or skills, not source comments.
- Keep source comments focused on non-obvious implementation constraints.

Rust:
- Workspace edition is Rust 2021, MSRV/rust-version is 1.86, toolchain is stable with `rustfmt` and `clippy` components.
- Use `cargo fmt --all` and `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings` as standard cleanup/checks.
- Default Rust verification is `cargo test --workspace --tests --verbose`; do not run bare `cargo test --workspace` unless doctests/examples are relevant or requested.
- When inline `#[cfg(test)] mod tests` exceeds roughly 400 lines, extract to sibling test files. For standalone files use `#[path = "name_tests.rs"] mod name_tests;`; for `mod.rs`, use sibling `tests.rs`.
- Tests must be hermetic by default. Do not depend on `.integration-tests/repos/<third-party>/...` or sibling external corpora unless gated behind a named Cargo feature.

TypeScript:
- Root TypeScript config uses `strict: true`, `module: NodeNext`, `moduleResolution: nodenext`, `jsx: preserve`, composite builds, and source/declaration maps.
- Unit tests are co-located as `*.spec.ts`; `packages/types/` has type-level tests with its own Vitest config.
- For new AI-assisted test files, the project testing skill asks for an `@ai-generated` JSDoc header; for individual AI-assisted tests in existing files, use a focused `// @ai-generated` comment.
- Type tests should include both positive assertions and `@ts-expect-error` negative assertions to guard against `any`, `unknown`, and `never` false positives.
- Internal TS helper types use `___VERTER___`; string-exported helper types use `$V_`.
- Script plugins use the local `definePlugin`/`ScriptPlugin` patterns in `packages/core/src/v5/process/script/plugins/`.

Formatting/linting:
- JS/TS formatting is via `oxfmt` (`pnpm run fmt` at root); lint-staged checks `*.{ts,js,mjs,cjs}` with `oxfmt --check --no-error-on-unmatched-pattern`.
- Rust formatting is `cargo fmt --all`; lint-staged checks `*.rs` with `cargo fmt --check --`.

Commit convention:
- Use conventional commits: `<type>(<scope>): <description>`.
- Common types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `release`.
- Common scopes: `core`, `napi`, `wasm`, `play`, `unplugin`, `lsp`, `types`, `ts`, `meta`, `ci`, `*`.