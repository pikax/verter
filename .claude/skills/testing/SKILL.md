---
name: testing
description: "Testing patterns, TDD workflow, TypeScript and Rust test conventions, sourcemap testing, and test execution hygiene for Verter"
---

# Testing Patterns & Conventions

For VS Code extension E2E fixtures, helpers API, and warm-session rules, see `/e2e-vscode-testing`.

## Server Cleanup

Always kill dev/preview servers or other long-running test processes when done — stale servers interfere with subsequent runs (e.g., Playwright's `reuseExistingServer: true` uses old builds).

Capture the PID when you start the server and terminate THAT PID. A port is a diagnostic, not a proof of ownership — `lsof -t -i:<port>` returns whoever holds the port, which may be the user's own server or another agent's, and killing by image name or pattern (`pkill -f node`, `taskkill /F /IM node.exe`, `Stop-Process -Name`) is never acceptable.

```bash
# Record the PID at spawn, terminate only that recorded tree:
pnpm --filter @verter/playground preview & SERVER_PID=$!   # capture at spawn

kill "$SERVER_PID"                                          # Unix — terminate only what you started
taskkill //F //T //PID "$(cat /proc/$SERVER_PID/winpid)"    # Windows — see both caveats below

# Or if using pnpm/npm scripts in a foreground shell, Ctrl+C the process
```

Two Windows caveats, both verified by running them rather than inferred from the flag names:

- **`$!` is the MSYS pid, not the Windows pid `taskkill` wants.** Passing `$SERVER_PID` straight to `taskkill` prints `ERROR: The process "…" not found`, exits 128, and kills nothing — hence the `/proc/<pid>/winpid` lookup.
- **`//T` does not reap descendants.** It terminates the named process (exit 0, `SUCCESS: … has been terminated`) while its children keep running. So `pnpm`'s child `vite`/`node` can outlive it. CONFIRM the server is actually gone rather than trusting the success line — `kill -0 "$SERVER_PID"`, or re-probe the port — and terminate any surviving child by its own recorded PID.

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

After TDD loop, run the full verification pass:

1. `cargo nextest run --workspace` — CANONICAL completeness gate; runs every workspace test target INCLUDING the ~25 verter_session integration binaries
2. `cargo test -p verter_session --tests` — shared-process surface for the verter_session integration suite
3. `cargo clippy --workspace -- -D warnings`
4. `cargo fmt --all --check`
5. `pnpm test` for TypeScript changes

Bare `cargo test --workspace --tests` silently SKIPS the verter_session integration suite (~4404 tests): `session_metrics` feature unification drops those binaries from the workspace test set, so the run reports green while never compiling them. Must NOT be used as the sole Rust gate — always run the `cargo nextest run --workspace` + `cargo test -p verter_session --tests` pair above.

Do not run bare `cargo test --workspace` (no `--tests`) by default — it also runs doctests and example builds, substantially slower. Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or explicitly requested.

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

### Testing Strategy

- **Unit tests**: Test individual plugins with minimal SFC snippets
- **Integration tests**: Test full transformation pipeline
- **Type tests**: Verify TypeScript inference (using `vitest --typecheck`)
- **Sourcemap tests**: Verify position mappings

### Architecture Guard Rule (MANDATORY)

Every new `CRITICAL` architecture rule must ship with an executable guard in the same change: static architecture guard, AST/source scanner with narrow allowlists, or a discriminating regression test that fails against old behavior. A rule without a guard is not durable enough for this repo's migration style.

When the guard cannot be automated immediately, the owning skill/doc must name it, explain the gap, and link the follow-up. Do not add prose-only critical rules that future changes can violate silently.

### Gate Integrity (MANDATORY)

A gate that does not run its intended surface must FAIL, not silently pass. Exit status 0 alone, a self-declared test universe, or a missing required-job result is FAIL — see `CLAUDE.md` → Verification Must Prove Execution for the normative rule.

Observed defect shapes, all of which reported success while testing nothing: a missing run-summary scored as a pass; a required CI job disabled with `if: false`; a helper timeout budget set ABOVE the timeout that kills it; a selector (`pnpm --filter`, a glob) matching no package and exiting 0; an unbuilt artifact under test; skips made vacuous by an absent fixture dependency; and a test script naming its spec files explicitly so a tracked spec sat in no gate at all.

**A plant that fails to apply reports a pass.** The same class has a fourth face, and it is the worst, because it sits inside the verification of the verification: a discrimination check plants a defect and expects RED, but the mutation silently no-ops (a `perl`/`sed` regex that does not match still exits 0), and the verification `grep` false-positives on a PRE-EXISTING occurrence of the planted string. The planted tree comes back green — and the check reports the test as discriminating. Prove the mutation is present, unique, and new in the source (`git diff` it) before trusting any planted run; never take a mutation command's exit code as proof it applied. If a planted run is green, the first hypothesis is that the plant failed — not that the test is weak, and never that the code is correct. See `/mom-cto-orchestration` → Plant Verification.

**Enforcement gap (tracked).** The rule is currently held by §1a and confirm JUDGMENT only. The planned guard is `gate_contract_integrity`: one registered suite exercising the exact canonical entry point against an independently tree-derived inventory, with per-surface negative controls for each shape above. Attestation alone is NOT sufficient — a receipt faithfully attests whatever incomplete universe the runner defines for itself, and a single global canary cannot detect an omitted unrelated spec; the design needs fresh execution attestation PLUS independent discovery parity PLUS per-surface mutation proof. Owed with it: a tree-derived verification-surface declaration (ownership, discovery roots, prerequisites, timeout relations, required jobs — not duplicated filenames), an attesting canonical driver emitting input-bound receipts, and an `if: always()` CI aggregator that fails on any missing/skipped/disabled/stale required-job receipt.

**The gap, its owner, and its resolution gate are a debt row, not a note.** Owner: the gate-integrity block. Resolution gate: that block's landing. The rows — including the promotion of `Verification Must Prove Execution` to `(CRITICAL)` as an acceptance criterion of that block — live in [`../../../docs/arch/gate-integrity-ledger.md`](../../../docs/arch/gate-integrity-ledger.md). Do not close the rule without them.

Live instances, verified in-tree:

- `.github/workflows/ci.yml:432` (`build-vscode-e2e`) and `:473` (`vscode-e2e`) both carry `if: false` — two required E2E jobs that produce no result, and a missing required-job result currently reads as a pass.
- `packages/vue-vscode/package.json` → `scripts.test` names 8 spec files explicitly, while the package has **21** tracked `*.spec.ts` files. The other **13** — including `activationGate.spec.ts` — are in no declared gate: root `pnpm test` is `pnpm -r --parallel run test`, which runs that same 8-file script, and CI runs only 4 named specs by path.

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
