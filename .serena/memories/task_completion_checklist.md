# Task Completion Checklist

For code changes:
1. Follow TDD: add/adjust a failing test first and verify it fails for the right reason.
2. Implement the minimum architecture-aligned fix.
3. Rerun the focused test(s) and verify green.
4. Refactor only while keeping tests green.
5. Run broader verification appropriate to the blast radius; for Rust changes default to `cargo test --workspace --tests --verbose`.
6. Run `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings` and `cargo fmt --all` for Rust work.
7. For TypeScript changes, run the relevant package tests and usually `pnpm test`; use root `pnpm vitest --run` or focused Vitest commands for narrower changes.
8. Run `pnpm install --frozen-lockfile` when dependency or lockfile state may be affected; this is also listed as an end-of-change check in `CLAUDE.md`.
9. Update owning documentation when public behavior, module paths, APIs, or durable architecture changes.
10. Self-review the implementation before declaring done: check for missed edge cases, incomplete migrations, unintended dual paths, stub tests, and phase/cutover archaeology in production comments.

Repository-specific constraints:
- Do not run bare `cargo test --workspace` by default; it includes doctests/examples and is slower. Use `cargo test --workspace --tests --verbose` unless doctests/examples changed or the user asked for them.
- Default-run tests must be hermetic. Do not introduce default tests that depend on external corpora or `.integration-tests/repos/...` unless feature-gated and excluded from the normal workspace test run.
- For VS Code extension or LSP changes, use automated tests: Vitest for pure logic, Mocha/E2E for LSP integration. Manual testing is not sufficient.
- For generated code, use `CodeTransform` operations rather than post-hoc string manipulation so sourcemaps remain valid.
- When replacing/refactoring, delete superseded code in the same change; do not leave compatibility shims or dual paths unless explicitly required by the approved plan.
- Agents are expected to record noteworthy issues/improvements/debt/docs gaps in `.feedback/feedback-{YYYY-MM-DD}-{short-id}.md` during work sessions.

Commit/publish hygiene:
- Use conventional commits: `<type>(<scope>): <description>`.
- Before committing, inspect `git status --short --branch` and avoid staging unrelated user changes.
- Do not revert user or unrelated work. Work with existing changes when they affect the task; ignore unrelated dirty files.