# Phase 9 — STOP / Surface to User

**Trigger:** §9.3 STOP CONDITIONS, first bullet:
> §9.1: `compile_batch` is on public NAPI API — STOP.

## Summary

`compile_batch` (Rust: `crates/verter_napi/src/lib.rs:2387`) is a
`#[napi]`-annotated public function that bypasses the host authority chain
and is re-exported as `compileBatch` in `packages/native/index.ts:128` and
`packages/native/index.js:175`. It is asserted to exist in
`packages/native/index.spec.ts:572` and is consumed by
`packages/benchmark/src/apple-to-apple.ts:24`.

Per §9.1 (r3 — Codex-9 + §16 update), the disposition is **NOT** the
"keep with `#[doc]` block" path. The production-grade fix is a
breaking-migration sub-plan, designed by a Plan-type subagent before
implementation.

## What this Phase 9 worker did

1. Read §9 of the cutover plan verbatim.
2. Verified anchors:
   - `compile_batch_files` at `crates/verter_napi/src/lib.rs:2345`.
   - `compile_batch` at `crates/verter_napi/src/lib.rs:2387`.
   - `use verter_compiler::compile::{compile as compile_sfc, ...};` at
     `crates/verter_napi/src/lib.rs:2314`.
3. Confirmed public-API surface:
   - `packages/native/index.ts:128` declares `export declare function
     compileBatch`.
   - `packages/native/index.js:125, 175` re-exports `compileBatch`.
   - `packages/native/index.spec.ts:572` asserts
     `expect(typeof native.compileBatch).toBe("function")`.
   - `packages/benchmark/src/apple-to-apple.ts:24` imports
     `compileBatch` from `@verter/native`.
4. Wrote `phase-09-breaking-sub-plan-needed.md` documenting the
   public-API surface, the architectural violation, and the proposed
   migration shape (with the Plan-type subagent invocation).
5. STOPped per §9.3 first bullet.

## What this Phase 9 worker did NOT do

- §9.2 (LSP/MCP dependency direction inventory) was not executed.
  Per §9.3, §9.1's STOP terminates Phase 9 execution before §9.2 runs.
  §9.2 can either be picked up by the next Phase 9 worker run after the
  breaking-migration sub-plan is approved, or promoted to its own
  dedicated phase. Marker `deferred` list records it.
- No production code under `crates/verter_napi/src/lib.rs` was modified.
- No host-backed compile API was designed or added — that is the
  Plan-type subagent's responsibility per the brief.
- No `#[doc]` block was added to `compile_batch`, since the brief's
  doc-block disposition only applies to the internal-only branch, which
  is the wrong branch here.

## Deferred items (per §0.5.1)

- `phase-09-breaking-sub-plan-needed`: design the `compile_batch`
  breaking migration via Plan-type subagent. The output target is
  `D:/tmp/verter-architecture-cutover-phase-09.md`.
- `phase-09.2-mcp-direction-inventory`: enumerate symbols crossing the
  LSP↔MCP boundary; informational only. Not blocked by §9.1's STOP, but
  §9.3 terminates the Phase 9 run, so this is deferred to the next run.

## Marker disposition

Phase 9's marker JSON records:

```json
{
  "status": "partial-deferred",
  "deferred": [
    "phase-09-breaking-sub-plan-needed",
    "phase-09.2-mcp-direction-inventory"
  ]
}
```
