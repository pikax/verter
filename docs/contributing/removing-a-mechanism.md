# Removing a Mechanism

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Deleting code is ordinary work here, not an exception. What is not ordinary is
deleting code and leaving the reader unable to tell whether the invariant it
carried survived. This page is the discipline for that: what a deletion must
name, what it may claim, and how to measure the result honestly.

## Name three things, in the same change

A deletion is complete when the change names all three:

1. **The removed mechanism** — the concrete thing that no longer exists: a
   guard, a scanner, a parallel table, a re-parse pass, a forwarding wrapper.
   Name it precisely enough that a reader can confirm it is gone.
2. **The surviving owner** — the single place the invariant now lives. If the
   answer is "nothing", say so explicitly and say why the invariant no longer
   applies; an unowned invariant is a silent regression, not a simplification.
3. **The preserved proof** — the assertion, compile-time boundary, or tool run
   that still fails when the invariant breaks. A deletion that also removes the
   only discriminating evidence is a coverage loss wearing a cleanup label.

Delete the mechanism, its exclusively-used helpers, fixtures, imports, module
wiring, and its references in owning docs **together**. A retired mechanism
whose allowlist, decoy fixture, or doc paragraph outlives it is worse than the
original: the next reader believes the guard is still there.

## Structural rails replace name-keyed scanners

The standing rule (see `CLAUDE.md`, "Landed guards are structural, never
name-keyed file scanners") is that a landed guard is a compiler, type-system, or
real-tool boundary — never a grep over the source tree for a spelled identifier.
When a scanner is retired, the invariant moves to a rail that cannot be evaded
by renaming:

| Invariant | Surviving owner | Preserved proof |
| --- | --- | --- |
| A hot carrier never hands out a materialized type body | The `NoTypeExpr` marker trait plus the sealed `OutputProjector` capabilities | The `assert_not_impl_any!` canary in `crates/verter_session/src/project_semantic_dispatch/output_materialization_guards.rs`; compile-time witnesses in `crates/verter_source_policy_gate/tests/cases/semantic_capability_witnesses.rs`; trybuild fixtures via `node scripts/compile-contracts.mjs` |
| One integration-test binary per crate | `scripts/check-integration-test-layout.mjs` plus its exact, stale-failing allowlist | The tool's live `cargo metadata` run, invoked once by `scripts/gate.mjs` and once by CI's `rust-test-build` job; discrimination fixtures in `scripts/check-integration-test-layout.test.mjs` |
| The browser surface never resurrects an external type engine | `capabilityForWasm` and the URL-state deserializer | `packages/playground/src/editor/wasmTsgoFailClosed.spec.ts` — asserts the capability record for every TypeScript major and that a persisted legacy selection deserializes to no engine at all |
| Every `(CRITICAL)` rule is enforced | The guards each rule names inline in `CLAUDE.md` | Those guards' own assertions. There is no rule-to-guard registry: membership in a list was never proof, and a registry that scanned doc headings only enforced spelling |

`// @ai-generated` markers still appear throughout the tree. They are
descriptive residue, not a requirement — nothing validates them, and a new file
does not need one.

## Consolidation keeps the real differences explicit

When two near-identical mechanisms collapse into one owner, that owner takes the
shared mechanics and the callers keep whatever genuinely differs. Read these
before consolidating something similar:

- `crates/verter_type_runtime/src/pending.rs` owns in-flight request
  bookkeeping and engine-silence watching for both external-TypeScript
  transports. Wire framing, the cancellation channel, and the error payload
  stayed with each protocol, because those are not the same thing.
- `crates/verter_type_runtime/src/codec.rs` owns line/column conversion. One
  index is built per immutable source version or response batch instead of
  re-scanning the source at every endpoint — while the 0-based and 1-based
  conventions, and the strict versus clamped variants, remain separate
  functions, since collapsing them would silently change fail-closed behavior.
- `crates/verter_lsp/src/provider_sync.rs` owns stale-provider path closing.
  Callers pass their own logging context instead of keeping duplicate loops,
  and no forwarding wrapper was left behind.
- `crates/verter_session/src/semantic_query_memo/` keeps one family memo, one
  production owner (`FlightCell`), and one batched admission path
  (`scc_publish`); the per-domain memo modules are payload read/write only.
- `crates/verter_compiler/src/template/code_gen/ssr/props_object.rs` holds
  keys, values, and source order as separate fields until a single `render`, so
  nothing has to re-read emitted text to find a key it just wrote.

## A declared check that no lane runs is not a check

The most expensive failure in this area is not a deleted test — it is a test
that exists, is committed, reads convincingly, and executes nowhere. A comment
saying "checked by X" is not a binding. The binding is a lane that runs X and
fails the build when X fails.

Before relying on a check, find the command that runs it. Two live examples of
what that looks like:

- **`packages/component-meta` public type contracts.** The package build
  excludes spec files and Vitest does not evaluate type-level assertions, so
  `test/*.test-d.ts` compiles only under `tsconfig.contract-tests.json`. That
  project is bound to `pnpm --filter @verter/component-meta run test:types`,
  which CI's *JS Build & Test* lane runs after building the TypeScript packages
  — the fixtures assert the built `dist` declarations as well as the source
  ones, so the build has to precede the check. Locally, run
  `pnpm --filter @verter/component-meta build` first, then the same script.
- **Lanes outside the canonical nextest surface.** Real provider suites, Svelte
  conformance, compile-fail fixtures, and proto freshness each have their own
  named command in the [Testing Guide](./testing.md). A green core gate is not
  evidence for any of them.

Confirm a check discriminates by making the invariant false, watching the check
go red, then restoring and watching it go green. Prove the mutation actually
applied — read the mutated line back — before trusting either result: a plant
that silently failed to apply reports a pass.

## Measuring the result

Report production, tests, comments, and generated data **separately**. A single
"lines removed" figure hides the case that matters most: test lines deleted
while production complexity is unchanged.

Use `git ls-files` as the file universe so untracked build output never enters a
count, then split:

- **Production versus tests.** Rust test files are anything under a `tests/`
  directory plus the `*_tests.rs` / `tests.rs` / `*test_support*.rs` siblings;
  JS/TS test files are `*.spec.*`, `*.test.*`, `*.test-d.*`, and `e2e/` trees.
  Inline `#[cfg(test)]` blocks inside a production `.rs` file count as
  production under a file-level split — say so rather than presenting the split
  as exact.
- **Comments.** Count comment-only lines separately from code. A module whose
  comment share rises after a deletion usually got clearer, not heavier.
- **Generated and fixture data.** Goldens, snapshots, corpora, `*.generated.*`
  files, and generator output directories. These move for reasons unrelated to
  complexity and must never be pooled with hand-written code.

Do not commit the numbers. A measurement is evidence for the change under
review; a committed count goes stale on the next commit and then gets "fixed"
by regenerating the record instead of by reading the tree. Record the figures
and the exact commands that produced them in the change's own evidence.

There is no line or percentage quota. A module that is still large after a
narrowing is a finding to report with its follow-up owner, not a reason to split
correct code into arbitrary files. The Rust gate prints an oversize advisory for
large production sources; it is informational and never affects the verdict.

## What a retirement may claim

- **Test and guard retirement** claims a maintenance and build-time benefit:
  fewer things to keep true, less to compile, a shorter path from a failure to
  its cause. Support it with comparable test or build timings where you have
  them.
- **It does not claim shipped runtime speed.** Tests are not in the product.
- **A production hot-path change** claims a runtime benefit only with
  equivalent-work evidence: the same work measured before and after, plus
  allocation and retention figures where that path owns them. Fewer lines is
  not a performance result.
- **Disabled or inapplicable paths stay zero-work.** A consolidation that gives
  a previously-free path something to do has regressed, even with every test
  green.

## Related pages

- [Testing Guide](./testing.md) — the command each lane owns.
- [Architecture Contracts](./architecture-contracts.md) — owner boundaries and
  the capability evidence basis.
- [Gate Performance](./gate-performance.md) — what the canonical gate measures,
  and what its telemetry deliberately does not decide.
