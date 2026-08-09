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

1. `node scripts/gate.mjs` — CANONICAL Rust gate. Builds the test universe ONCE via `cargo nextest archive` (single compile, no second-command recompile), then runs the two DEBUG surfaces from the same artifacts — SURFACE 1 = `cargo nextest run --workspace` (per-test process isolation, every workspace test target including the ~25 verter_session integration binaries); SURFACE 2 = the verter_session libtest binaries executed DIRECTLY (in-process / multi-test-per-process, the same direct surface as `cargo test -p verter_session --tests`). SURFACE 2 runs those binaries under the workspace-unified `session_metrics` feature set (ON), intentionally replacing the old package-scoped default-feature (`session_metrics` OFF) rebuild rather than reproducing its feature config — no test target the old pair compiled is dropped. SURFACE 3 then builds a SECOND `--workspace` archive with `--cargo-profile no-debug-assertions` and RUNS `package(verter_session) + package(verter_scheduler)` from it — see "Shipped-cfg surface" below. Before the archive build it runs a freshness-tooling preflight: it ensures the workspace `buf` + `oxfmt` binaries are present (auto-running `pnpm install --frozen-lockfile` inside the mutex/timeout/stall machinery when the `node_modules/.bin` shims are missing), then VERDICT-GATES the `cases::typeinfo_proto_ts_freshness::*` byte-pin tolerance on that outcome — tooling present/installed ⇒ tolerance OFF, so a freshness failure is a HARD gate failure (exit 1), NOT PASS-WITH-TOLERATED; a deterministic install failure (e.g. frozen-lockfile mismatch) ⇒ a LOUD setup failure (exit 127), never silently tolerated (when an install is attempted — both `node_modules/.bin/{buf,oxfmt}` shims already present ⇒ the preflight returns already-present and no install runs); when pnpm is not resolvable AND `buf` is not resolvable the Rust byte-pin pair SKIPS gracefully and PASSES, so the gate reports an ORDINARY PASS (no FAIL line) — the verdict-gated tolerance flips ON there only as a LATENT safety net that would surface PASS-WITH-TOLERATED solely in the unusual case the pair emitted a tolerated FAIL despite `buf` being absent. `oxfmt` absence NEVER grants tolerance — with `buf` present, a missing `oxfmt` is a LOUD setup failure (exit 127), not a degraded run. Run it with `node_modules` present (the normal path) so the byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD FAIL (a real stale-binding regression to regenerate + commit) — PASS-WITH-TOLERATED is NEVER the regression signal on a normal machine, and on a buf-less runner the pair yields an ordinary PASS via the skip, not PASS-WITH-TOLERATED. See `docs/arch/gate-performance.md`.
2. `cargo clippy --workspace -- -D warnings`
3. `cargo check --workspace --release` — the only thing in the loop that compiles the REAL release profile (opt-level 3 + fat LTO); the gate's shipped-cfg surface uses the cheap `no-debug-assertions` profile and surfaces 1–2 are debug. `debug_assert!` gates on `cfg!`, a RUNTIME constant, so its body still name-resolves in release: a `#[cfg(debug_assertions)]` helper called inside one is an E0425 in every release build (napi and wasm artifacts included) while compiling clean in debug. It is a CHECK and RUNS NO TESTS, so it cannot observe the runtime half of that class — a state mutation written inside a `debug_assert!` argument compiles fine and silently never executes in a shipped build. Gate SURFACE 3 is what covers that half. Mirrored in CI by the `rust-build-configs` job.
4. `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` — host clippy cannot see target-gated code, and the `wasm32-wasip1`/`wasip2` clippy jobs cover the SEPARATE `extensions/lapce` + `extensions/zed` manifests, not this one. Same `rust-build-configs` job in CI.
5. `cargo fmt --all --check`
6. `pnpm test` for TypeScript changes

Confirm `cargo clippy --version` reports the `rust-toolchain.toml`-pinned version before trusting
2–4. Clippy output from a different toolchain is not evidence about the one CI runs.

The gate runner also emits an advisory warning for each non-exempt production Rust source above 1,500
lines, formatted as `path (N lines)`. This scan is informational only: its findings do not enter either
surface analyzer, the failure accumulator, or the final gate verdict.

**Build-prerequisite preflight — fail-closed, and the FIRST step of gate mode.** Parts of the Rust suite
load artifacts cargo does not build. The real-provider suites spawn the pinned tsserver with
`--globalPlugins @verter/typescript-plugin --pluginProbeLocations packages/vue-vscode/node_modules`; that
probe dir is a pnpm symlink to `packages/typescript-plugin`, whose `main` is `dist/index.js` — a `tsc -b`
OUTPUT that `pnpm install` does NOT produce. With the symlink present and the dist absent, tsserver loads
no plugin, cannot resolve `.vue`/`.svelte` carriers, and ~64 `*_tsserver` tests fail with `TS2307: Cannot
find module './Comp.vue' or its corresponding type declarations.` — sixty-four opaque failures that read
exactly like a compiler regression. So before the freshness preflight, before cargo, and before any test,
the gate **loads** that plugin entry in a child process and, on any load failure, exits 127 with the marker
`BUILD-PREREQUISITE MISSING`, naming the probe target, the load error, the producing packages, and the
producer command.

- **The oracle is a real load, not a file list.** `require()` of the probe directory — the same resolution
  tsserver performs — in a child process (`runBuildPrerequisiteLoadProbe`), fail-closed on every non-zero
  shape (structured load error, spawn error, signal, timeout, unparseable output). A stat list would be a
  mirror of the emit graph and would drift: the plugin entry eagerly requires its emitted helpers
  (`dist/helpers/carrierStore.js` among them) and `@verter/language-shared`'s entry re-exports a dozen
  emitted siblings, so a tree with **both `index.js` present and one helper missing** satisfies every stat
  and still throws inside tsserver. The load also covers the half-built case the plugin's tsconfig allows:
  `noEmitOnError` is off, so `tsc -b` on the plugin alone emits a dist while failing with `TS2307: Cannot
  find module '@verter/language-shared'`.
- **The probe runs under tsserver's environment, not the gate's.** `TsserverTypeProvider::spawn` strips
  `CHILD_PROCESS_ENV_DENYLIST` (`crates/verter_type_runtime/src/tsserver/ipc.rs` — `NODE_OPTIONS`,
  `VSCODE_INSPECTOR_OPTIONS`, `ELECTRON_RUN_AS_NODE`) before launching node, so an inheriting probe would
  have strictly more influence than the process it speaks for. That gap is exploitable, not theoretical:
  measured, `NODE_OPTIONS=--require=<preload>` with a preload patching `Module._load` to return a dummy for
  `process.argv[1]` made the probe exit 0 and report loaded while tsserver still failed on a missing
  helper. The probe reads that denylist **out of the Rust call site** (`parseTsserverEnvDenylist`) rather
  than restating it, so the two cannot drift; a generated mirror was rejected because its freshness test
  lives in the Rust suite the probe runs *before*. If the const cannot be found or parsed the probe fails
  closed as `environment-unknown`. It strips exactly that denylist and nothing more — equivalence, not
  maximal hardening: a var tsserver also inherits influences the real load identically.
- **The timeout is a HARD bound, sized by the gate's deadline.** `spawnSync`'s default `killSignal` is
  SIGTERM, which a child can trap — measured, a child trapping SIGTERM with an open handle left the parent
  blocked for its full 25s lifetime and then returned status 0, i.e. a hang *and* a false positive. The
  probe kills with `SIGKILL` (no graceful phase: the child's whole job is one `require()`, so there is
  nothing to flush — unlike `runContainedStep`, which escalates because it reaps whole build trees), and its
  budget is `probeBudgetMs(deadline, now)` — the smaller of a 60s cap and the gate's own remaining
  wallclock, so it cannot outlive the `--timeout` it sits inside while holding the single-flight mutex.
  Honest limit: this kills the direct child only; a module that spawns a detached grandchild on require
  would leak it, the same limitation the contained-step runner carries.
- **Failure classes are typed, not string-matched.** The probe returns a `reason`
  (`module-not-found` / `load-error` / `timeout` / `spawn-error` / `signalled` / `unknown-exit` /
  `environment-unknown`). Only `module-not-found` means "this tree was never built"; every other class means
  the probe could not *answer*. Callers that gate behavior on the prerequisite must branch on `reason` —
  see `(xix)` below, where skipping on any failure would green-skip a scenario whose artifacts are present.
- **What it does not prove: freshness.** A dist that loads but was emitted from an older commit passes.
  That is a separate problem and is deliberately out of scope — not an oversight.
- **Two packages produce the closure**, and they are the producer command's scope: the plugin, plus
  `@verter/language-shared`. `@verter/native` is deliberately excluded — the plugin's
  `"files": ["src/index.ts"]` leaves out `src/tsc/`, its only consumer, and no Rust test loads a `.node`.
- **Produce them** with `pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build`
  (pnpm orders multi-filter recursive scripts topologically). NOT `pnpm build` (native + LSP + wasm + every
  TS package) and NOT `--filter @verter/typescript-plugin...` — the trailing ellipsis selects the package
  AND ITS DEPENDENCIES, dragging in `@verter/native`'s `napi build --release`. This is also the step
  `ci.yml`'s `rust-test` and `release.yml`'s `test` job run before the gate.
- **It never builds for you and never skips.** Building implicitly would make the verdict depend on a
  mutation the gate performed; skipping would restore the silent pass — with no install at all the affected
  tests SKIP and the gate goes green having proven nothing, the "unexpected prerequisite skips" half of
  Verification Must Prove Execution. `--prepare` is exempt: it builds the archive and runs no test.
- **Discrimination** is proven by `(GB9)` in `scripts/gate-selftest.mjs`, which drives the REAL production
  CLI (a byte-copy rooted in a synthetic git repo holding a miniature of the package graph, so the gate
  keeps its zero test seams and no developer tree is mutated) in six directions: nothing built / plugin
  entry missing / language-shared missing / **a transitively-required helper missing while both entries are
  present** ⇒ 127 before the freshness preflight and before cargo, with the refusal reporting
  `MODULE_NOT_FOUND` rather than a probe that could not answer; everything built ⇒ SATISFIED and the run
  proceeds. Every plant is stat-proven applied and re-stated after the run. Three properties are exercised
  with real subprocesses rather than injected result shapes, because they are the ways a bad tree could
  still pass: `(GB9.1b)` runs a **real SIGTERM-trapping child** and asserts the bound holds, the reason is
  `timeout`, and no process survives; `(GB9.1c)` runs a **real forged `NODE_OPTIONS` preload** and asserts
  it cannot fake a load, with a helper-present control proving the env was sanitized rather than broken, and
  a launcher-deleted leg proving `environment-unknown` fails closed; `(GB9.1d)`/`(GB9.1e)` pin the denylist
  parser's fail-closed shapes and the deadline clamp.
- **`(xix)` declares its precondition, narrowly.** That scenario drives the real gate against the real repo
  expecting it to reach cargo, so on a tree without the prerequisites it would hit this 127 and report a
  meaningless failure — and its verdict would depend on the very state under test. It now measures the state
  and emits a TRUE skip (counted in SKIP, never PASS) **only** when `reason === "module-not-found"`. Any
  other class — EPERM, timeout, an unrelated plugin throw, an unreadable launcher — FAILS, because
  `finish()` exits 0 whenever FAIL is zero, so skipping on an infrastructure failure would silently retire a
  scenario whose artifacts are present.

**Shipped-cfg surface (SURFACE 3) — what it covers, and what it does not.**

`debug_assert!` does not evaluate its argument when `debug_assertions` is off, and `#[cfg(debug_assertions)]`
items do not exist there. Every shipped artifact (the LSP binary, napi, wasm) is built that way. So a state
mutation written inside a `debug_assert!` argument — the shipped shape being
`debug_assert!(session.commit_completed())`, where the call performs the state transition — runs in every
debug test and in NO shipped build.

- **Nothing else in the repo sees it.** Surfaces 1 and 2 are debug builds, so the effect happens and the
  tests pass. `cargo check --workspace --release` compiles the shipped cfg but RUNS NOTHING, so it cannot
  observe a runtime no-op. Only executing tests with `debug_assertions` off makes it observable.
- **How.** A second `cargo nextest archive --workspace --cargo-profile no-debug-assertions` (the profile is
  declared in the workspace `Cargo.toml`: `debug_assertions` off + `overflow-checks` off, dev codegen
  otherwise), then `cargo nextest run` over that archive with
  `-E 'package(verter_session) + package(verter_scheduler)'`. Same discovery machinery, same watchdog, same
  failure analyzer as surface 1 — a variant selects only the Cargo profile.
- **It is also a compile gate.** A dependency's item gated on `debug_assertions` is a profile accident: the
  predicate is evaluated per compilation unit, so a dependent crate's test code can reference an item that
  vanishes under another profile. Under this profile that is a COMPILE error in the gate rather than a
  shipped-build surprise. Cross-crate and cross-target test hooks therefore declare availability with a
  cargo feature (`verter_scheduler`'s and `verter_session`'s `test-support`), never with `debug_assertions`.
- **Selection integrity.** The surface fails closed (exit 127) if the filterset's packages are absent from
  the shipped-cfg archive listing, or if the run executes zero tests.
- **NOT covered, explicitly.** It is not an optimised build: the profile inherits dev codegen (opt-level 0,
  no LTO, many codegen units), so optimisation-, inlining- and LTO-dependent behaviour is out of scope. It
  runs under nextest process isolation only — there is no in-process shipped-cfg equivalent of surface 2 —
  and it runs the filterset above, not the whole archive, so a `debug_assertions`-dependent regression in a
  package outside that filterset is not covered. The real `release` profile is compiled only by
  `cargo check --workspace --release`, which runs no tests.
- **Cost.** A second whole-workspace compile: a different profile is a different unit hash, so no artifact
  is shared with the dev archive.

Without Node, or to debug one surface in isolation, run the two underlying debug surfaces directly: `cargo nextest run --workspace` then `cargo test -p verter_session --tests`. Neither covers the shipped-cfg class; the closest manual equivalent is `cargo nextest run --workspace --cargo-profile no-debug-assertions -E 'package(verter_session) + package(verter_scheduler)'`. Run the gate with `node_modules` present (e.g. `pnpm install --frozen-lockfile` first in a fresh worktree) so the freshness-tooling preflight is a no-op and the `cases::typeinfo_proto_ts_freshness::*` byte-pin runs genuinely — with the tooling present a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), not tolerated. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS; the verdict-gated tolerance flips ON there only as a latent safety net (PASS-WITH-TOLERATED appears solely if the pair somehow emitted a tolerated FAIL despite `buf` being absent, which the skip does not). `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure).

Bare `cargo test --workspace --tests` silently SKIPS the verter_session integration suite (~4404 tests): `session_metrics` feature unification drops those binaries from the workspace test set, so the run reports green while never compiling them. Must NOT be used as the sole Rust gate — run `node scripts/gate.mjs` (which runs both surfaces from one archive) or the `cargo nextest run --workspace` + `cargo test -p verter_session --tests` pair directly.

Do not run bare `cargo test --workspace` (no `--tests`) by default — it also runs doctests and example builds, substantially slower. Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or explicitly requested.

### §1a Mutation Recipes

For every NEW or CHANGED correctness-bearing test, guard, or refusal, record a reversible mutation recipe: verify the starting SHA; plant the mutation; run the named guarding test and require the expected failure (RED); restore; verify a clean original SHA; run the green test; run an unplanted control that stays GREEN. Persist commands and results. Read every new test body; reject stubs, always-true assertions, and non-discriminating characterization. The independent confirmer executes each recipe again; sampling is forbidden.

Canonical in-tree examples of fully self-contained recipes (in-memory plant → expected verdict → trivial restore + GREEN control): the Vue structural-conformance discriminator `crates/verter_vue_conformance/tests/cases/conformance_discriminator.rs` (cosmetic-PASS vs behavioral-FAIL mutations on committed goldens, each plant proven applied) and the Svelte oracle discriminator in `crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs`.

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
