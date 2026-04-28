# Phase 9 — Thin Adapters — Report

**Branch:** `wt/phase-09-thin-adapters`
**Base commit at spawn:** `6b1cd1e967bca2d0993c67bae220608e4429fecd`
**Status:** `partial-deferred` (per §0.5.1)

## Summary

Phase 9 §9.1 STOP triggered: `compile_batch` (in
`crates/verter_napi/src/lib.rs:2387`) is on the public NAPI API surface
and bypasses the host authority chain. Per §9.1 r3 (Codex-9 + §16
update), this requires a breaking-migration sub-plan, not a `#[doc]`
annotation. Worker wrote the deferral artifacts and STOPped per §9.3
first bullet. §9.2 (LSP/MCP dependency direction inventory) was not
executed and is also deferred.

## §9.1 — `compile_batch` audit

### Anchors verified

- `compile_batch_files` at `crates/verter_napi/src/lib.rs:2345`
- `compile_batch` at `crates/verter_napi/src/lib.rs:2387` (annotated
  `#[napi]`, returns `Result<Vec<BatchResult>>`)
- `use verter_compiler::compile::{compile as compile_sfc, ...};` at
  `crates/verter_napi/src/lib.rs:2314` — direct compiler import that
  bypasses the host

### Classification: PUBLIC NAPI API

`compile_batch` is on the public NAPI API surface:

- `pub fn compile_batch` with `#[napi]` annotation in Rust.
- `export declare function compileBatch(...)` in
  `packages/native/index.ts:128` (typed exports).
- `module.exports.compileBatch = compileBatch;` in
  `packages/native/index.js:175`.
- Asserted to be a function in
  `packages/native/index.spec.ts:572`.
- Consumed by `packages/benchmark/src/apple-to-apple.ts:24,213,266`
  (the apple-to-apples comparison benchmark vs. Vize's
  `compileSfcBatch`).

### Architectural violation

`compile_batch` calls `verter_compiler::compile::compile` directly via
`rayon::par_iter`. It bypasses the entire authority chain:

- No `VerterHost` (no scheduler-owned parsing).
- No `IndexedReady` artifact produced.
- No `ProjectTypeStore` consultation.
- No cross-file semantic queries.
- Each file gets its own bumpalo allocator per Rayon thread (no
  reuse, no caching).

The Rust source comment at lines 2306-2309 explicitly documents the
bypass intent:

```rust
// compile_batch() is a pure stateless parallel compiler: no VerterHost, no
// caching. Each file gets its own bumpalo Allocator per Rayon thread.
// This matches Vize's compileSfcBatch() API for a fair benchmark comparison.
```

### Deferral artifact (commit 1)

`phase-09-breaking-sub-plan-needed.md` — documents:

1. The public-API surface (Rust + TS + JS layers, plus
   consumers).
2. The architectural violation (authority-chain bypass).
3. The proposed breaking-migration shape — two design directions
   (Option A: extend `VerterHost`; Option B: shared substrate
   utility), commit-by-commit sequence, deprecation window.
4. The Plan-type subagent invocation specified by §9.1
   (lines 3748-3766 of the cutover plan).

`phase-09-stuck.md` — documents the STOP per §9.3, what was/was not
done, and the deferral list.

## §9.2 — LSP/MCP dependency direction inventory

**Not executed.** §9.3 STOP at §9.1 terminates Phase 9 execution
before §9.2 runs. Recorded in marker `deferred` list as
`phase-09.2-mcp-direction-inventory`.

## Verification gates (§0.6.3)

All gates pass on the deferral-only commit:

| Gate | Result |
| ---- | ------ |
| `cargo test --workspace --tests --verbose` | 7417 passed, 0 failed (14 test-result blocks) |
| `cargo clippy --workspace -- -D warnings` | clean (exit 0); pre-existing meta_resolve.rs warnings owned by Phase 11a |
| `cargo fmt --all --check` | clean |
| `pnpm install --frozen-lockfile` | clean |
| `cargo test -p verter_session --test correctness` | 11 passed, 0 failed, 1 ignored (pre-existing) |

## Commits

| # | SHA | Message |
| - | --- | ------- |
| 1 | (pending) | `docs(napi): defer compile_batch to breaking-migration sub-plan` |
| 2 | (pending — last commit) | `chore(orchestrator): mark phase 09 complete` |

## Deferred items

- `phase-09-breaking-sub-plan-needed` — design and execute the
  `compile_batch` breaking migration (NAPI public-API removal +
  host-backed replacement). Owner: Plan-type subagent + follow-up
  worker.
- `phase-09.2-mcp-direction-inventory` — enumerate symbols crossing
  the LSP↔MCP boundary. Informational; can be picked up by a later
  Phase 9 re-run or a dedicated phase.

## Snapshot drift

`none`. No snapshot files were modified — Phase 9 §9.1 deferral does
not touch fixtures, and §9.2 is informational-only (would only
produce a `.md` document if run).
