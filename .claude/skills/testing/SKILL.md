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

### Pinned Vue Macro Runtime Oracle

The official Vue macro baseline is generated only from the repository-pinned
local `@vue/compiler-sfc@3.5.34`; tests must not fetch compiler output from the
network. `scripts/vue-macro-runtime-oracle/oracle-lib.mjs` parses compiled
JavaScript and compares normalized runtime facts instead of carrier formatting.
Schema v2 records compiler/profile provenance, constructor order, `skipCheck`,
literal-safe defaults/`defaultKind`, and `typePresent` so an omitted type cannot
collapse with explicit `type: null`. Profile fixtures cover development,
production, and production custom-element output. A
`verter-complete-extension` row records the official
Unknown baseline and may be refined only by a canonical `Complete` Verter
result.

Regenerate and verify with:

```bash
node scripts/gen-vue-macro-runtime-oracle.mjs
node scripts/gen-vue-macro-runtime-oracle.mjs --check
node --test scripts/vue-macro-runtime-oracle/oracle.test.mjs
```

Never hand-edit
`crates/verter_session/tests/fixtures/vue_macro_runtime_oracle.json`; the
generator owns it. The canonical gate runs both drift verification and the
oracle unit tests.

### End-of-change Checks

Outside the orchestration landing-train lifecycle — a local change NOT driven as a train — run the canonical Rust pair after the change, per the repo End-of-change Checks. The full workspace suite is the canonical completeness gate.

Any change driven THROUGH the landing-train lifecycle — including a single substantial train (even a one-slice train) — uses the tiered gating: during slice implementation and fix cycles, targeted runs (changed tests + affected crates + a conservative reverse-dependency closure) are ITERATION EVIDENCE ONLY, and a selector that cannot prove the affected closure MUST fall back to full-workspace coverage for that run (still iteration evidence, never landing evidence); the canonical pair runs at exactly the two lifecycle points — after the final content change on the rebased, landing-frozen train tree, and again independently at post-land confirm. Targeted success is never landing evidence, and the standalone clause above never lets a train-driven change skip the frozen-tree final gate.

The canonical full verification pass:

1. `node scripts/gate.mjs` — CANONICAL Rust gate. Builds the test universe ONCE via `cargo nextest archive` (single compile, no second-command recompile), then runs BOTH surfaces from the same artifacts — SURFACE 1 = `cargo nextest run --workspace` (per-test process isolation, every workspace test target including the ~25 verter_session integration binaries); SURFACE 2 = the verter_session libtest binaries executed DIRECTLY (in-process / multi-test-per-process, the same direct surface as `cargo test -p verter_session --tests`). SURFACE 2 runs those binaries under the workspace-unified `session_metrics` feature set (ON), intentionally replacing the old package-scoped default-feature (`session_metrics` OFF) rebuild rather than reproducing its feature config — no test target the old pair compiled is dropped. Before the archive build it runs a freshness-tooling preflight: it ensures the workspace `buf` + `oxfmt` binaries are present (auto-running `pnpm install --frozen-lockfile` inside the mutex/timeout/stall machinery when the `node_modules/.bin` shims are missing), then VERDICT-GATES the `cases::typeinfo_proto_ts_freshness::*` byte-pin tolerance on that outcome — tooling present/installed ⇒ tolerance OFF, so a freshness failure is a HARD gate failure (exit 1), NOT PASS-WITH-TOLERATED; a deterministic install failure (e.g. frozen-lockfile mismatch) ⇒ a LOUD setup failure (exit 127), never silently tolerated (when an install is attempted — both `node_modules/.bin/{buf,oxfmt}` shims already present ⇒ the preflight returns already-present and no install runs); when pnpm is not resolvable AND `buf` is not resolvable the Rust byte-pin pair SKIPS gracefully and PASSES, so the gate reports an ORDINARY PASS (no FAIL line) — the verdict-gated tolerance flips ON there only as a LATENT safety net that would surface PASS-WITH-TOLERATED solely in the unusual case the pair emitted a tolerated FAIL despite `buf` being absent. `oxfmt` absence NEVER grants tolerance — with `buf` present, a missing `oxfmt` is a LOUD setup failure (exit 127), not a degraded run. Run it with `node_modules` present (the normal path) so the byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD FAIL (a real stale-binding regression to regenerate + commit) — PASS-WITH-TOLERATED is NEVER the regression signal on a normal machine, and on a buf-less runner the pair yields an ordinary PASS via the skip, not PASS-WITH-TOLERATED. See `docs/arch/gate-performance.md`.
2. `cargo clippy --workspace -- -D warnings`
3. `cargo fmt --all --check`
4. `pnpm test` for TypeScript changes

Without Node, or to debug one surface in isolation, run the two underlying surfaces directly: `cargo nextest run --workspace` then `cargo test -p verter_session --tests`. Run the gate with `node_modules` present (e.g. `pnpm install --frozen-lockfile` first in a fresh worktree) so the freshness-tooling preflight is a no-op and the `cases::typeinfo_proto_ts_freshness::*` byte-pin runs genuinely — with the tooling present a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), not tolerated. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS; the verdict-gated tolerance flips ON there only as a latent safety net (PASS-WITH-TOLERATED appears solely if the pair somehow emitted a tolerated FAIL despite `buf` being absent, which the skip does not). `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure).

Bare `cargo test --workspace --tests` silently SKIPS the verter_session integration suite (~4404 tests): `session_metrics` feature unification drops those binaries from the workspace test set, so the run reports green while never compiling them. Must NOT be used as the sole Rust gate — run `node scripts/gate.mjs` (which runs both surfaces from one archive) or the `cargo nextest run --workspace` + `cargo test -p verter_session --tests` pair directly.

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

Default-run tests must depend only on locally-vendored fixtures. The canonical run (`node scripts/gate.mjs`, i.e. its two underlying surfaces `cargo nextest run --workspace` + `cargo test -p verter_session --tests`) must compile and pass on a fresh checkout without any `.integration-tests/repos/<third-party>/...` clones, sibling repositories, or other external corpora present alongside the workspace.

When needing fixtures from a third-party project (e.g., `nuxt-ui` Vue corpus), vendor a snapshot into the consuming crate's `tests/<feature>/fixtures/` and refer to them with `include_str!("./fixtures/...")` or path-based loaders. Preserve upstream license attribution in sibling `LICENSE.md` and `README.md` for provenance.

Tests requiring live external corpora (e.g., periodic drift detectors comparing the vendored snapshot against the upstream submodule) must be gated behind a Cargo feature naming the corpus dependency:

```toml
# crates/<crate>/Cargo.toml
[features]
external-corpus = []
```

```rust
#![cfg(feature = "external-corpus")]
//! Optional drift detector — gated so the default gate run
//! (`node scripts/gate.mjs`) stays hermetic.
```

Guard `external_corpus_paths_not_present_outside_gated_tests` (in `crates/verter_session/tests/cases/architecture_guards.rs`) rejects `include_str!` / `include!` / path-string references to `.integration-tests/repos/...` from any test file not gated behind such a feature.
