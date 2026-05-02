---
name: testing
description: "Testing patterns, TDD workflow, TypeScript and Rust test conventions, sourcemap testing, and test execution hygiene for Verter"
---

# Testing Patterns & Conventions

For VS Code extension E2E fixtures, helpers API, and warm-session rules, see `/e2e-vscode-testing`.

## Server Cleanup

**IMPORTANT**: After starting any dev server, preview server, or other long-running process for testing purposes, **always kill it when done**. This prevents stale servers from interfering with subsequent test runs (e.g., Playwright's `reuseExistingServer: true` will use a stale server serving old builds).

```bash
# After finishing with a server, kill it
# If started in background, use the process ID or port:
kill $(lsof -t -i:4173)   # Unix
taskkill //F //PID <pid>   # Windows

# Or if using pnpm/npm scripts, Ctrl+C the process
```

## Test Output Best Practices

When running E2E tests or test suites where you need to inspect output, **redirect output to a temp file first**, then grep/read the file. This avoids re-running expensive builds and tests just to search for different patterns:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
# Then search as needed:
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full test suite each time you need different output
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

## TypeScript Test Patterns

**Test locations**: Unit tests are co-located as `*.spec.ts` next to source files. Type tests in `packages/types/` use `vitest --typecheck`.

**AI-generated tests**: Add appropriate comments indicating AI assistance:

```typescript
// For new test files, add a JSDoc at the top:
/**
 * @ai-generated - This test file was generated with AI assistance.
 * Brief description of what the tests cover.
 */

// For individual tests in existing files:
// @ai-generated - Tests X functionality with Y scenarios
it("does something", () => {
  /* ... */
});
```

**Sourcemap testing** (see `macros.map.spec.ts`):

```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

**Type testing best practices** (`packages/types/`):

- Always include **both** a positive assertion and a `@ts-expect-error` negative assertion
- This prevents `any`/`unknown`/`never` types from silently passing tests

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion - type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error - Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

## Rust Test Patterns

### Test File Organization

When a Rust source file's inline `#[cfg(test)] mod tests` block exceeds ~400 lines, extract tests to a separate sibling file. Two patterns:

**For standalone files** (e.g., `analysis.rs`):

```rust
// In analysis.rs — replace the inline #[cfg(test)] mod tests { ... } block:
#[cfg(test)]
#[path = "analysis_tests.rs"]
mod analysis_tests;
```

**For `mod.rs` files** (e.g., `ide/template/mod.rs`):

```rust
// In mod.rs — loads tests.rs from the same directory:
#[cfg(test)]
mod tests;
```

The extracted file contains the module contents directly — `use super::*;`, helpers, and `#[test]` functions. No wrapping `mod tests { }` block.

### TDD Workflow

1. Write failing tests first
2. Implement the minimum code to pass
3. Run the relevant tests and verify they pass
4. Refactor while keeping tests green

### End-of-change Checks

After the TDD loop, run the full verification pass:

1. `cargo test --workspace --tests --verbose` (default workspace-wide Rust verification; skips doctests/examples)
2. `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings`
3. `cargo fmt --all`
4. `pnpm test` for TypeScript changes

Do not run bare `cargo test --workspace` by default in this repo. It also runs doctests and example builds, which are substantially slower than the normal verification loop. Run doctests only when rustdoc examples changed or the user explicitly asks for them.

### Test Validation Pattern

All codegen tests must validate generated JS syntax:

```rust
let result = compile_sfc(source);
let tpl = result.template.unwrap();
// Parse generated code with OXC to verify valid JS
let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
assert!(parsed.errors.is_empty(), "JS parse error: {:?}\n{}", parsed.errors, tpl.code);
```

### Testing Strategy

- **Unit tests**: Test individual plugins with minimal SFC snippets
- **Integration tests**: Test full transformation pipeline
- **Type tests**: Verify TypeScript inference (using `vitest --typecheck`)
- **Sourcemap tests**: Verify position mappings


### Test Hermeticity (MANDATORY)

Default-run tests must depend only on locally-vendored fixtures. The `cargo test --workspace --tests --verbose` invocation must compile and pass on a fresh checkout without any `.integration-tests/repos/<third-party>/...` clones, sibling repositories, or other external corpora present alongside the workspace.

When you need fixtures sourced from a third-party project (e.g., the nuxt-ui Vue corpus), vendor a snapshot of the upstream files into the consuming crate's `tests/<feature>/fixtures/` directory and refer to them with `include_str!("./fixtures/...")` or path-based loaders. Preserve upstream license attribution in a sibling `LICENSE.md` and `README.md` for provenance.

Tests that genuinely require live external corpora (e.g., periodic drift detectors comparing the vendored snapshot against the upstream submodule) must be gated behind a Cargo feature whose name names the corpus dependency:

```toml
# crates/<crate>/Cargo.toml
[features]
external-corpus = []
```

```rust
#![cfg(feature = "external-corpus")]
//! Optional drift detector — gated so default `cargo test --workspace`
//! stays hermetic.
```

The architecture guard `external_corpus_paths_not_present_outside_gated_tests` (in `crates/verter_session/tests/architecture_guards.rs`) rejects `include_str!` / `include!` / path-string references to `.integration-tests/repos/...` from any test file that is not gated behind such a feature. A regression that re-introduces a non-hermetic dependency surfaces here at test time.
