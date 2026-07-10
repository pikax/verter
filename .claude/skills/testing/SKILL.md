---
name: testing
description: "Testing patterns, TDD workflow, TypeScript and Rust test conventions, sourcemap testing, and test execution hygiene for Verter"
---

# Testing Patterns & Conventions

For VS Code extension E2E fixtures, helpers API, and warm-session rules, see `/e2e-vscode-testing`.

## Server Cleanup

Always kill dev/preview servers or other long-running test processes when done — stale servers interfere with subsequent runs (e.g., Playwright's `reuseExistingServer: true` uses old builds).

```bash
# After finishing with a server, kill it
# If started in background, use the process ID or port:
kill $(lsof -t -i:4173)   # Unix
taskkill //F //PID <pid>   # Windows

# Or if using pnpm/npm scripts, Ctrl+C the process
```

## Test Output Best Practices

Redirect output to a temp file, then grep — avoids re-running expensive builds:

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

**Test locations**: Unit tests co-located as `*.spec.ts` next to source. Type tests in `packages/types/` use `vitest --typecheck`.

**AI-generated tests**: Add comments indicating AI assistance:

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

- Always include both a positive assertion and a `@ts-expect-error` negative assertion — prevents `any`/`unknown`/`never` types from silently passing.

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

Extracted file contains module contents directly — `use super::*;`, helpers, and `#[test]` fns. No wrapping `mod tests { }` block.

### TDD Workflow

1. Write failing tests first
2. Implement minimum code to pass
3. Run relevant tests, verify pass
4. Refactor while keeping tests green

### End-of-change Checks

Outside the orchestration landing-train lifecycle — a local change NOT driven as a train — run the canonical Rust pair after the change, per the repo End-of-change Checks. The full workspace suite is the canonical completeness gate.

Any change driven THROUGH the landing-train lifecycle — including a single substantial train (even a one-slice train) — uses the tiered gating: during slice implementation and fix cycles, targeted runs (changed tests + affected crates + a conservative reverse-dependency closure) are ITERATION EVIDENCE ONLY, and a selector that cannot prove the affected closure MUST fall back to full-workspace coverage for that run (still iteration evidence, never landing evidence); the canonical pair runs at exactly the two lifecycle points — after the final content change on the rebased, landing-frozen train tree, and again independently at post-land confirm. Targeted success is never landing evidence, and the standalone clause above never lets a train-driven change skip the frozen-tree final gate.

The canonical full verification pass:

1. `cargo nextest run --workspace` — CANONICAL completeness gate; runs every workspace test target INCLUDING the ~25 verter_session integration binaries
2. `cargo test -p verter_session --tests` — shared-process surface for the verter_session integration suite
3. `cargo clippy --workspace -- -D warnings`
4. `cargo fmt --all --check`
5. `pnpm test` for TypeScript changes

Bare `cargo test --workspace --tests` silently SKIPS the verter_session integration suite (~4404 tests): `session_metrics` feature unification drops those binaries from the workspace test set, so the run reports green while never compiling them. Must NOT be used as the sole Rust gate — always run the `cargo nextest run --workspace` + `cargo test -p verter_session --tests` pair above.

Do not run bare `cargo test --workspace` (no `--tests`) by default — it also runs doctests and example builds, substantially slower. Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or explicitly requested.

### §1a Mutation Recipes

For every NEW or CHANGED correctness-bearing test, guard, or refusal, record a reversible mutation recipe: verify the starting SHA; plant the mutation; run the named guarding test and require the expected failure (RED); restore; verify a clean original SHA; run the green test; run an unplanted control that stays GREEN. Persist commands and results. Read every new test body; reject stubs, always-true assertions, and non-discriminating characterization. The independent confirmer executes each recipe again; sampling is forbidden.

### Timeout Is Never a Pass

A timeout or incomplete run is never green and never presumed environmental. Rerun the timed-out test in isolation with an adequate timeout and no co-resident heavy work: if it clears → environmental (retain both artifacts); if it repeats → collect hang diagnostics; if classification stays ambiguous → HARD FAIL. The advertised slow-timeout must match the configured one — `.config/nextest.toml` advertises ~60s but configures 5s×3, killing valid tests around 15s on an 8GB host; fix that mismatch rather than tolerating false timeouts. Genuinely long tests get explicit per-test overrides.

### Enum-variant ripple (silent catch-all absorption)

When changing a variant of a widely-matched enum (`SemanticQueryKey`, `TypeExpr`, `WorkKind`, `EmitOp`, etc.), `cargo check` does NOT flag a `_ =>` catch-all that silently absorbs the changed variant. Grep every `match` on the enum for `_ =>` / `..` wildcards (and every TS `default:` switch) and confirm each intends the new behavior. Distinguish ANALYZER-IR consumers (which see the raw analyzer variants) from DISPATCH-RAISED consumers (which see the collapsed forms produced at `raise.rs`) — the same logical change may need edits in both.

### Test Validation Pattern

All codegen tests must validate generated JS syntax:

```rust
let result = compile_sfc(source);
let tpl = result.template.unwrap();
// Parse generated code with OXC to verify valid JS
let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
assert!(parsed.errors.is_empty(), "JS parse error: {:?}\n{}", parsed.errors, tpl.code);
```

### Never Hand-Edit Generated Goldens

Regenerate goldens from their authoritative source and record the source-manifest identity in the review evidence packet. A hand-edited golden is a defect, not a fixture update.

### Testing Strategy

- **Unit tests**: Test individual plugins with minimal SFC snippets
- **Integration tests**: Test full transformation pipeline
- **Type tests**: Verify TypeScript inference (using `vitest --typecheck`)
- **Sourcemap tests**: Verify position mappings

### Architecture Guard Rule (MANDATORY)

Every new `CRITICAL` architecture rule must land with primary EXECUTABLE enforcement in the same change. Primary architecture enforcement uses type or capability boundaries, dependency checks, AST-aware analysis, or a discriminating behavioral guard that fails against old behavior. Textual/substring scanning may exist only as a secondary retired-symbol tripwire and cannot establish architectural compliance. Prose plus a future follow-up is insufficient — a rule without primary executable enforcement is not durable enough for this repo's migration style.

### Test Hermeticity (MANDATORY)

Default-run tests must depend only on locally-vendored fixtures. The canonical run (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`) must compile and pass on a fresh checkout without any `.integration-tests/repos/<third-party>/...` clones, sibling repositories, or other external corpora present alongside the workspace.

When needing fixtures from a third-party project (e.g., `nuxt-ui` Vue corpus), vendor a snapshot into the consuming crate's `tests/<feature>/fixtures/` and refer to them with `include_str!("./fixtures/...")` or path-based loaders. Preserve upstream license attribution in sibling `LICENSE.md` and `README.md` for provenance.

Tests requiring live external corpora (e.g., periodic drift detectors comparing the vendored snapshot against the upstream submodule) must be gated behind a Cargo feature naming the corpus dependency:

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

Guard `external_corpus_paths_not_present_outside_gated_tests` (in `crates/verter_session/tests/architecture_guards.rs`) rejects `include_str!` / `include!` / path-string references to `.integration-tests/repos/...` from any test file not gated behind such a feature.
